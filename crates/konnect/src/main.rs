mod config;
mod install;
mod manifest;
mod transaction_cli;
mod transport;

use anyhow::Result;
use config::{Config, TransportMode};
use konnect_core::mcp::handler::McpHandler;
use std::io::IsTerminal;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // ─── CLI argument parsing (minimal, no clap dependency) ─────────
    let args: Vec<String> = std::env::args().collect();

    // ─── Subcommand dispatch (install, uninstall, status, skill) ────
    match args.get(1).map(String::as_str) {
        Some("init") => return install::run_install(),
        Some("uninstall") => return install::run_uninstall(),
        Some("status") => return install::print_status(),
        Some("skill") => {
            let name = args.get(2).map(String::as_str).unwrap_or("");
            return install::print_skill_content(name);
        }
        Some("transaction") => return transaction_cli::run(&args[2..]),
        Some("--version") | Some("-V") => {
            println!("konnect {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            return Ok(());
        }
        _ => {}
    }

    // ─── Double-click detection ─────────────────────────────────────
    // If stdin is a terminal (user double-clicked the .exe), run friendly install.
    // If stdin is piped (Claude launched us as MCP server), start server.
    if std::io::stdin().is_terminal() {
        return install::run_double_click_install();
    }

    // ─── Auto-install on first MCP launch (safety net) ──────────────
    if install::needs_install() {
        let _ = install::run_install_silent();
    }

    // --config <path>: load config from specified file
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|pos| args.get(pos + 1))
        .map(std::path::PathBuf::from);

    let (config, ipc_address_source) = if let Some(ref path) = config_path {
        // KiCAD launches the server this way (with KICAD_API_SOCKET set), so
        // the env fallback for a blank ipc_address must apply here too (#39).
        let mut c = Config::load_from(path)?;
        let source = c.apply_env_fallbacks()?;
        (c, source)
    } else {
        Config::load_with_ipc_source()?
    };

    // ─── Initialize tracing (stderr only — stdout is MCP protocol) ──
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    fmt::Subscriber::builder()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    info!("Konnect v{} starting", env!("CARGO_PKG_VERSION"));

    match ipc_address_source {
        config::IpcAddressSource::Configured => {
            info!(
                "ipc_address: using configured value as-is -> {}",
                config.ipc_address
            );
        }
        config::IpcAddressSource::EnvVar => {
            info!(
                "ipc_address: resolved from KICAD_API_SOCKET -> {}",
                config.ipc_address
            );
        }
        config::IpcAddressSource::PlatformDefault => {
            info!(
                "ipc_address: using platform default -> {}",
                config.ipc_address
            );
        }
    }

    // Resolve kicad_cli/kicad_binary against the machine when the config
    // left them at a bare name (no explicit override). An explicit config
    // value always wins and is passed through untouched; a bare name that
    // fails to resolve anywhere is also passed through untouched, so the
    // existing "Failed to spawn kicad-cli" error stays intact (INV4: no
    // silent fallback without a validator).
    let kicad_cli_default = if cfg!(target_os = "windows") {
        "kicad-cli.exe"
    } else {
        "kicad-cli"
    };
    let kicad_binary_default = if cfg!(target_os = "windows") {
        "kicad.exe"
    } else {
        "kicad"
    };
    let kicad_cli = resolve_and_log("kicad_cli", &config.kicad_cli, kicad_cli_default, true);
    let kicad_binary = resolve_and_log(
        "kicad_binary",
        &config.kicad_binary,
        kicad_binary_default,
        false,
    );

    let server_config = konnect_core::tools::ServerConfig {
        kicad_cli,
        kicad_binary,
        ipc_address: config.ipc_address.clone(),
        project_dir: config.project_dir.clone(),
        jlcpcb_db_path: config.jlcpcb_db_path.clone(),
        auto_load_toolsets: config.auto_load_toolsets,
        mode: config.mode,
    };
    let handler =
        McpHandler::new_with_agent_provider(server_config, config.local_provider()?).await?;

    match config.transport {
        TransportMode::Stdio => {
            transport::stdio::run_stdio(handler).await?;
        }
        TransportMode::Http => {
            transport::http::run_http(handler, &config.http_address).await?;
        }
        TransportMode::Both => {
            let handler_http = handler.clone();
            let http_addr = config.http_address.clone();
            let http_task = tokio::spawn(async move {
                transport::http::run_http(handler_http, &http_addr)
                    .await
                    .expect("HTTP transport failed");
            });
            let stdio_task = tokio::spawn(async move {
                transport::stdio::run_stdio(handler)
                    .await
                    .expect("STDIO transport failed");
            });
            tokio::select! {
                _ = http_task => {},
                _ = stdio_task => {},
            }
        }
    }

    Ok(())
}

/// Resolve one configured binary path (`kicad_cli` or `kicad_binary`)
/// against the machine and log, once, which branch answered. Shares
/// resolution with `install::detect_kicad()` via `install::resolve_binary`
/// so the server and `konnect status`/`init` never disagree.
fn resolve_and_log(
    label: &str,
    configured: &str,
    default_name: &str,
    try_registry: bool,
) -> String {
    let local_appdata = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let standard_paths = install::kicad_standard_paths(default_name, local_appdata.as_deref());

    #[cfg(target_os = "windows")]
    let (path, source) = if try_registry {
        install::resolve_binary(
            configured,
            default_name,
            &standard_paths,
            install::detect_kicad_from_registry,
        )
    } else {
        install::resolve_binary(configured, default_name, &standard_paths, || None)
    };
    #[cfg(not(target_os = "windows"))]
    let (path, source) = {
        let _ = try_registry;
        install::resolve_binary(configured, default_name, &standard_paths, || None)
    };

    let resolved = path.to_string_lossy().to_string();
    match source {
        install::KicadCliSource::Configured => {
            info!("{label}: using configured value \"{configured}\" as-is");
        }
        install::KicadCliSource::Path => {
            info!("{label}: resolved \"{configured}\" via PATH -> {resolved}");
        }
        install::KicadCliSource::StandardPath => {
            info!("{label}: found at standard install path -> {resolved}");
        }
        install::KicadCliSource::Registry => {
            info!("{label}: found via Windows registry -> {resolved}");
        }
    }
    resolved
}

fn print_help() {
    println!("Konnect v{}", env!("CARGO_PKG_VERSION"));
    println!("MCP server for KiCAD EDA with embedded skills and agents.\n");
    println!("USAGE:");
    println!("  konnect                  Start MCP server (pipe) or install (TTY)");
    println!("  konnect init             Install skills, agents, and hooks");
    println!("  konnect uninstall        Remove all installed files");
    println!("  konnect status           Show install state");
    println!("  konnect skill <name>     Print skill content (for hooks)");
    println!("  konnect transaction status <project-dir>");
    println!("  konnect transaction recover <project-dir>");
    println!("  konnect transaction abandon <project-dir> <id> --force");
    println!("  konnect --config <path>  Start server with config file");
    println!("  konnect --version        Print version");
    println!("  konnect --help           This message");
}
