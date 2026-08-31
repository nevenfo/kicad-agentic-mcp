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
    let document_context = parse_document_context(&args)?;

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
    let handler = McpHandler::new_with_agent_provider_and_document_context(
        server_config,
        config.local_provider()?,
        document_context,
    )
    .await?;

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

fn parse_document_context(args: &[String]) -> Result<konnect_ipc::DocumentContext> {
    let mut indexes = args
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "--document-type").then_some(index));
    let Some(index) = indexes.next() else {
        return Ok(konnect_ipc::DocumentContext::Auto);
    };
    if indexes.next().is_some() {
        anyhow::bail!("--document-type may be supplied only once");
    }

    match args.get(index + 1).map(String::as_str) {
        Some("pcb") => Ok(konnect_ipc::DocumentContext::Pcb),
        Some("schematic") => Ok(konnect_ipc::DocumentContext::Schematic),
        Some(value) => {
            anyhow::bail!("invalid --document-type '{value}': expected 'pcb' or 'schematic'")
        }
        None => anyhow::bail!("--document-type requires 'pcb' or 'schematic'"),
    }
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
    use konnect_core::kicad_locate::{kicad_standard_paths, resolve_binary, KicadCliSource};

    let local_appdata = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    let standard_paths = kicad_standard_paths(default_name, local_appdata.as_deref());

    #[cfg(target_os = "windows")]
    let (path, source) = if try_registry {
        resolve_binary(configured, default_name, &standard_paths, || {
            konnect_core::kicad_locate::detect_kicad_from_registry(default_name)
        })
    } else {
        resolve_binary(configured, default_name, &standard_paths, || None)
    };
    #[cfg(not(target_os = "windows"))]
    let (path, source) = {
        let _ = try_registry;
        resolve_binary(configured, default_name, &standard_paths, || None)
    };

    let resolved = path.to_string_lossy().to_string();
    match source {
        KicadCliSource::Configured => {
            info!("{label}: using configured value \"{configured}\" as-is");
        }
        KicadCliSource::Path => {
            info!("{label}: resolved \"{configured}\" via PATH -> {resolved}");
        }
        KicadCliSource::StandardPath => {
            info!("{label}: found at standard install path -> {resolved}");
        }
        KicadCliSource::Registry => {
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
    println!("  konnect --document-type <pcb|schematic>  Bind KiCad plugin context");
    println!("  konnect --version        Print version");
    println!("  konnect --help           This message");
}

#[cfg(test)]
mod tests {
    use super::parse_document_context;
    use konnect_ipc::DocumentContext;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn document_context_requires_a_valid_single_value() {
        assert_eq!(
            parse_document_context(&args(&["konnect"])).unwrap(),
            DocumentContext::Auto
        );
        assert_eq!(
            parse_document_context(&args(&["konnect", "--document-type", "pcb"])).unwrap(),
            DocumentContext::Pcb
        );
        assert_eq!(
            parse_document_context(&args(&["konnect", "--document-type", "schematic"])).unwrap(),
            DocumentContext::Schematic
        );
        assert!(parse_document_context(&args(&["konnect", "--document-type"])).is_err());
        assert!(parse_document_context(&args(&[
            "konnect",
            "--document-type",
            "pcb",
            "--document-type"
        ]))
        .is_err());
        assert!(parse_document_context(&args(&["konnect", "--document-type", "unknown"])).is_err());
    }

    #[test]
    fn plugin_actions_pass_their_scoped_document_context() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../plugin/plugin.json")).unwrap();
        let actions = manifest["actions"].as_array().unwrap();

        for (scope, document_type) in [("pcb", "pcb"), ("schematic", "schematic")] {
            let action = actions
                .iter()
                .find(|action| action["scopes"] == serde_json::json!([scope]))
                .unwrap_or_else(|| panic!("missing plugin action for {scope}"));
            assert_eq!(
                action["args"],
                serde_json::json!(["--document-type", document_type])
            );
        }
    }
}
