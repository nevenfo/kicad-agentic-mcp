//! First-run installer for Konnect.
//!
//! Handles:
//! - `init` — full install with console output
//! - `uninstall` — remove all installed files and hook entries
//! - `status` — show install state with [+]/[-] markers
//! - `skill <name>` — print a skill's markdown to stdout (for hook integration)
//! - Silent install on first MCP launch (no stdout, stderr logging only)
//! - KiCAD auto-detection on Windows

use crate::manifest::{AGENTS, HOOK_SKILLS, SKILLS};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Full install with console output. Called by `init` subcommand or double-click.
pub fn run_install() -> Result<()> {
    println!("Installing Konnect skills, agents, and hooks...\n");

    // Skills
    let skills_dir = claude_skills_dir()?;
    let mut skill_count = 0;
    for skill in SKILLS {
        let dest = skills_dir.join(skill.name);
        fs::create_dir_all(&dest)?;
        fs::write(dest.join("SKILL.md"), skill.content)?;

        // Reference files
        if !skill.references.is_empty() {
            let refs_dir = dest.join("references");
            fs::create_dir_all(&refs_dir)?;
            for (filename, content) in skill.references {
                fs::write(refs_dir.join(filename), content)?;
            }
        }
        skill_count += 1;
        println!("  [+] Skill: {}", skill.name);
    }

    // Agents
    let agents_dir = claude_agents_dir()?;
    fs::create_dir_all(&agents_dir)?;
    let mut agent_count = 0;
    for agent in AGENTS {
        fs::write(agents_dir.join(agent.filename), agent.content)?;
        agent_count += 1;
        println!("  [+] Agent: {}", agent.filename);
    }

    // Hooks
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();
    let hook_count = patch_claude_settings(&exe_str)?;
    if hook_count > 0 {
        println!(
            "  [+] Hooks: {} entries patched into settings.json",
            hook_count
        );
    } else {
        println!("  [=] Hooks: already installed (no changes)");
    }

    // KiCAD detection
    if let Some(kicad_path) = detect_kicad() {
        println!("\n  [+] Found KiCAD at: {}", kicad_path.display());
    } else {
        println!("\n  [-] KiCAD not found in standard locations");
        println!("      Set kicad_cli path in your config file manually");
    }

    // Write marker
    let data = data_dir()?;
    fs::create_dir_all(&data)?;
    fs::write(data.join(".installed"), env!("CARGO_PKG_VERSION"))?;

    println!(
        "\nDone: {} skills, {} agents, {} hooks installed.",
        skill_count, agent_count, hook_count
    );
    Ok(())
}

/// Silent install — no stdout output (safe for MCP pipe mode).
/// Logs to stderr via tracing.
pub fn run_install_silent() -> Result<()> {
    // Skills
    let skills_dir = claude_skills_dir()?;
    for skill in SKILLS {
        let dest = skills_dir.join(skill.name);
        fs::create_dir_all(&dest)?;
        fs::write(dest.join("SKILL.md"), skill.content)?;
        if !skill.references.is_empty() {
            let refs_dir = dest.join("references");
            fs::create_dir_all(&refs_dir)?;
            for (filename, content) in skill.references {
                fs::write(refs_dir.join(filename), content)?;
            }
        }
    }

    // Agents
    let agents_dir = claude_agents_dir()?;
    fs::create_dir_all(&agents_dir)?;
    for agent in AGENTS {
        fs::write(agents_dir.join(agent.filename), agent.content)?;
    }

    // Hooks
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();
    let _ = patch_claude_settings(&exe_str);

    // Marker
    let data = data_dir()?;
    fs::create_dir_all(&data)?;
    fs::write(data.join(".installed"), env!("CARGO_PKG_VERSION"))?;

    eprintln!(
        "[konnect] Silent install complete: {} skills, {} agents",
        SKILLS.len(),
        AGENTS.len()
    );
    Ok(())
}

/// Remove all installed files and hook entries.
pub fn run_uninstall() -> Result<()> {
    println!("Uninstalling Konnect skills, agents, and hooks...\n");

    // Skills
    let skills_dir = claude_skills_dir()?;
    for skill in SKILLS {
        let dest = skills_dir.join(skill.name);
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
            println!("  [-] Removed skill: {}", skill.name);
        }
    }

    // Agents
    let agents_dir = claude_agents_dir()?;
    for agent in AGENTS {
        let dest = agents_dir.join(agent.filename);
        if dest.exists() {
            fs::remove_file(&dest)?;
            println!("  [-] Removed agent: {}", agent.filename);
        }
    }

    // Hooks — remove our entries from settings.json
    remove_hooks_from_settings()?;
    println!("  [-] Removed hook entries from settings.json");

    // Marker
    let data = data_dir()?;
    let marker = data.join(".installed");
    if marker.exists() {
        fs::remove_file(&marker)?;
    }

    println!("\nDone.");
    Ok(())
}

/// Print install status with [+]/[-] markers.
pub fn print_status() -> Result<()> {
    println!("Konnect v{} — Install Status\n", env!("CARGO_PKG_VERSION"));

    let skills_dir = claude_skills_dir()?;
    println!("Skills (~/.claude/skills/):");
    for skill in SKILLS {
        let exists = skills_dir.join(skill.name).join("SKILL.md").exists();
        let marker = if exists { "+" } else { "-" };
        println!("  [{}] {}", marker, skill.name);
    }

    let agents_dir = claude_agents_dir()?;
    println!("\nAgents (~/.claude/agents/):");
    for agent in AGENTS {
        let exists = agents_dir.join(agent.filename).exists();
        let marker = if exists { "+" } else { "-" };
        println!("  [{}] {}", marker, agent.filename);
    }

    println!("\nHooks (~/.claude/settings.json):");
    let settings_path = claude_settings_path();
    if settings_path.exists() {
        let raw = fs::read_to_string(&settings_path).unwrap_or_default();
        for hook in HOOK_SKILLS {
            let exists = raw.contains(hook.name);
            let marker = if exists { "+" } else { "-" };
            println!("  [{}] {} ({})", marker, hook.name, hook.event);
        }
    } else {
        for hook in HOOK_SKILLS {
            println!("  [-] {} ({})", hook.name, hook.event);
        }
    }

    // KiCAD detection
    println!("\nKiCAD:");
    if let Some(path) = detect_kicad() {
        println!("  [+] Found: {}", path.display());
    } else {
        println!("  [-] Not found in standard locations");
    }

    let data = data_dir()?;
    let marker = data.join(".installed");
    if marker.exists() {
        let ver = fs::read_to_string(&marker).unwrap_or_default();
        println!("\nInstall marker: v{}", ver.trim());
    } else {
        println!("\nInstall marker: not present (never installed)");
    }

    Ok(())
}

/// Print a skill's content to stdout. Used by hooks:
/// `konnect.exe skill <name>` outputs markdown that Claude Code
/// injects before/after a tool call.
pub fn print_skill_content(name: &str) -> Result<()> {
    // Check hook skills first (they have short inline content)
    for hook in HOOK_SKILLS {
        if hook.name == name {
            print!("{}", hook.content);
            return Ok(());
        }
    }

    // Check regular skills
    for skill in SKILLS {
        if skill.name == name {
            print!("{}", skill.content);
            return Ok(());
        }
    }

    eprintln!("Unknown skill: {}", name);
    std::process::exit(1);
}

/// Check if install has been completed.
pub fn needs_install() -> bool {
    match data_dir() {
        Ok(d) => !d.join(".installed").exists(),
        Err(_) => false,
    }
}

/// Friendly double-click install: shows banner, runs install, prints config snippet.
pub fn run_double_click_install() -> Result<()> {
    println!("===========================================");
    println!("  Konnect v{}", env!("CARGO_PKG_VERSION"));
    println!("  First-time Setup");
    println!("===========================================\n");

    run_install()?;

    // Print MCP config snippet
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().replace('\\', "\\\\");

    println!("\n-------------------------------------------");
    println!("Add this to your Claude MCP config:");
    println!("-------------------------------------------\n");
    println!(r#"  "konnect": {{"#);
    println!(r#"    "command": "{}","#, exe_str);
    println!(r#"    "env": {{ "RUST_LOG": "info" }}"#);
    println!(r#"  }}"#);

    println!("\nConfig locations:");
    println!("  Claude Desktop: %APPDATA%\\Claude\\claude_desktop_config.json");
    println!("  Claude Code:    .mcp.json in your project root");
    println!("\nAfter editing the config, restart Claude.\n");

    println!("Press Enter to close...");
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
    Ok(())
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not locate home directory")
}

fn data_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".konnect"))
}

fn claude_skills_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".claude").join("skills"))
}

fn claude_agents_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".claude").join("agents"))
}

fn claude_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

/// Idempotent hook patching: adds hook entries to `~/.claude/settings.json`.
/// Returns the number of NEW entries added (0 if all already existed).
fn patch_claude_settings(exe_str: &str) -> Result<usize> {
    let path = claude_settings_path();
    fs::create_dir_all(path.parent().unwrap())?;

    let raw = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        "{}".to_string()
    };
    let mut settings: serde_json::Value = serde_json::from_str(&raw)?;

    let hooks_obj = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("hooks field is not an object")?;

    let mut added = 0;

    for hook in HOOK_SKILLS {
        let event_arr = hooks_obj
            .entry(hook.event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("hook event field is not an array")?;

        // Idempotent: skip if a hook with this matcher already exists
        let already_exists = event_arr.iter().any(|h| {
            h.get("matcher")
                .and_then(|m| m.as_str())
                .map(|m| m.contains(hook.name))
                .unwrap_or(false)
        });

        if !already_exists {
            // Use the exe path with escaped backslashes for the command
            let exe_escaped = exe_str.replace('\\', "\\\\");
            let entry = serde_json::json!({
                "matcher": hook.tool_matcher,
                "hooks": [{
                    "type": "command",
                    "command": format!("{} skill {}", exe_escaped, hook.name)
                }]
            });
            event_arr.push(entry);
            added += 1;
        }
    }

    fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    Ok(added)
}

/// Remove only our hook entries from settings.json (leave other hooks intact).
fn remove_hooks_from_settings() -> Result<()> {
    let path = claude_settings_path();
    if !path.exists() {
        return Ok(());
    }

    let raw = fs::read_to_string(&path)?;
    let mut settings: serde_json::Value = serde_json::from_str(&raw)?;

    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for hook in HOOK_SKILLS {
            if let Some(event_arr) = hooks_obj.get_mut(hook.event).and_then(|a| a.as_array_mut()) {
                event_arr.retain(|h| {
                    let is_ours = h
                        .get("hooks")
                        .and_then(|hooks| hooks.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|h| h.get("command"))
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("konnect"))
                        .unwrap_or(false);
                    !is_ours
                });
            }
        }
    }

    fs::write(&path, serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}

// Binary-location logic (KicadCliSource, resolve_binary, kicad_standard_paths,
// detect_kicad_from_registry) lives in `konnect_core::kicad_locate` — shared
// with `find_kicad_binary` in `konnect-core::tools::verification` since
// `konnect` depends on `konnect-core`, not the reverse.
use konnect_core::kicad_locate::{kicad_standard_paths, resolve_binary, KicadCliSource};

/// Auto-detect a KiCAD installation (used by `init`/`status`, which have no
/// user-configured value to respect). Checks standard per-platform paths for
/// kicad-cli, then the registry on Windows.
pub fn detect_kicad() -> Option<PathBuf> {
    let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let cli_name = if cfg!(target_os = "windows") {
        "kicad-cli.exe"
    } else {
        "kicad-cli"
    };
    let standard_paths = kicad_standard_paths(cli_name, local_appdata.as_deref());

    #[cfg(target_os = "windows")]
    let (path, source) = resolve_binary(cli_name, cli_name, &standard_paths, || {
        konnect_core::kicad_locate::detect_kicad_from_registry(cli_name)
    });
    #[cfg(not(target_os = "windows"))]
    let (path, source) = resolve_binary(cli_name, cli_name, &standard_paths, || None);

    match source {
        KicadCliSource::Configured => None,
        _ => Some(path),
    }
}
