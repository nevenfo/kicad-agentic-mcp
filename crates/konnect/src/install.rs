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
use std::path::{Path, PathBuf};

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

/// Where a resolved `kicad-cli` path came from — logged once at startup so a
/// user can tell which KiCAD install was picked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KicadCliSource {
    /// The configured value was used as-is: either an explicit (non-bare)
    /// path, which always wins and is never replaced, or a bare name that
    /// nothing below could resolve — kept so the OS's own "not found" error
    /// stays intact (INV4: no silent fallback without a validator).
    Configured,
    /// A bare filename resolved via the `PATH` environment variable.
    Path,
    /// One of the known per-platform install-prefix candidates.
    StandardPath,
    /// The Windows registry (machine key or the per-user uninstall key).
    Registry,
}

/// Resolve `kicad_cli`/`kicad_binary` against the machine, in priority order:
/// 1. an explicit configured value — i.e. anything other than the compiled-in
///    bare `default_name` for this platform — never touched, not even
///    checked against `PATH`; a config file that names a binary is trusted
///    verbatim, so a deliberately-wrong value still fails loudly at spawn
///    time instead of being silently substituted;
/// 2. the bare default name, if it resolves via `PATH`;
/// 3. known install-prefix candidates (`standard_paths`, e.g. from
///    [`kicad_standard_paths`]);
/// 4. the Windows registry, via `registry_probe` (e.g.
///    [`detect_kicad_from_registry`]).
///
/// `standard_paths` and `registry_probe` are parameters (not globals) so
/// tests can inject a temp-dir prefix instead of depending on the real
/// machine. Returns the configured value unchanged, tagged `Configured`,
/// when nothing else matches — callers must keep spawning that value so a
/// real "file not found" surfaces rather than being swallowed.
pub fn resolve_binary(
    configured: &str,
    default_name: &str,
    standard_paths: &[PathBuf],
    registry_probe: impl FnOnce() -> Option<PathBuf>,
) -> (PathBuf, KicadCliSource) {
    let configured_path = PathBuf::from(configured);

    if configured != default_name {
        return (configured_path, KicadCliSource::Configured);
    }

    if resolves_on_path(configured) {
        return (configured_path, KicadCliSource::Path);
    }

    for path in standard_paths {
        if path.exists() {
            return (path.clone(), KicadCliSource::StandardPath);
        }
    }

    if let Some(path) = registry_probe() {
        return (path, KicadCliSource::Registry);
    }

    (configured_path, KicadCliSource::Configured)
}

/// Whether `name` (a bare filename) resolves to an existing file on `PATH`.
fn resolves_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| dir.join(name).is_file())
}

/// Standard per-platform KiCAD install-prefix candidates for `filename`
/// (`kicad-cli.exe`/`kicad-cli`, or `kicad.exe`/`kicad` for the GUI binary —
/// both live in the same per-version `bin`/bundle directory, so the
/// directory list is shared and only the filename differs).
/// `windows_local_appdata` is injectable (used by tests); production calls
/// pass the real `%LOCALAPPDATA%` value.
pub fn kicad_standard_paths(filename: &str, windows_local_appdata: Option<&Path>) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // Newest version first, and every prefix of one version before any
        // prefix of an older one: a per-user KiCad 10 has to win over a
        // system-wide KiCad 9, which listing all the 10.0 roots and then all
        // the 9.0 roots got backwards. The per-user prefix is where the KiCAD
        // installer puts KiCad when it is run without admin rights.
        let mut paths: Vec<PathBuf> = Vec::new();
        for ver in ["10.0", "9.0"] {
            for root in [
                r"C:\KiCad",
                r"C:\Program Files\KiCad",
                r"C:\Program Files (x86)\KiCad",
            ] {
                paths.push(PathBuf::from(root).join(ver).join("bin").join(filename));
            }
            // %LOCALAPPDATA%\Programs\KiCad\<ver>\bin\
            if let Some(local_appdata) = windows_local_appdata {
                paths.push(
                    local_appdata
                        .join("Programs")
                        .join("KiCad")
                        .join(ver)
                        .join("bin")
                        .join(filename),
                );
            }
        }
        paths
    }

    #[cfg(target_os = "macos")]
    {
        let _ = windows_local_appdata; // unused on this platform
        let mut paths = vec![
            PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS").join(filename),
            PathBuf::from("/usr/local/bin").join(filename),
        ];
        if let Ok(home) = std::env::var("HOME") {
            // Per-user install (KiCad.app dragged into ~/Applications)
            paths.push(
                PathBuf::from(home)
                    .join("Applications/KiCad/KiCad.app/Contents/MacOS")
                    .join(filename),
            );
        }
        paths
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = windows_local_appdata; // unused on this platform
        vec![
            PathBuf::from("/usr/bin").join(filename),
            PathBuf::from("/usr/local/bin").join(filename),
        ]
    }
}

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
    let (path, source) = resolve_binary(
        cli_name,
        cli_name,
        &standard_paths,
        detect_kicad_from_registry,
    );
    #[cfg(not(target_os = "windows"))]
    let (path, source) = resolve_binary(cli_name, cli_name, &standard_paths, || None);

    match source {
        KicadCliSource::Configured => None,
        _ => Some(path),
    }
}

/// Query the Windows registry for a KiCAD install location:
/// - `HKLM\SOFTWARE\KiCad\<ver>` (default value, `/ve`) — set by an
///   admin/machine-wide install;
/// - the per-user uninstall key (`InstallLocation`), HKCU then HKLM — what a
///   per-user install (the installer run without admin rights) actually
///   writes.
#[cfg(target_os = "windows")]
pub fn detect_kicad_from_registry() -> Option<PathBuf> {
    if let Some(install_dir) = registry_default_value(r"HKLM\SOFTWARE\KiCad\10.0") {
        let cli_path = install_dir.join("bin").join("kicad-cli.exe");
        if cli_path.exists() {
            return Some(cli_path);
        }
    }

    for hive in ["HKCU", "HKLM"] {
        let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\KiCad 10.0");
        if let Some(install_dir) = registry_named_value(&key, "InstallLocation") {
            let cli_path = install_dir.join("bin").join("kicad-cli.exe");
            if cli_path.exists() {
                return Some(cli_path);
            }
        }
    }

    None
}

/// Query a registry key's default (unnamed) value via `reg.exe` (avoids a
/// `winreg` dependency).
#[cfg(target_os = "windows")]
fn registry_default_value(key: &str) -> Option<PathBuf> {
    use std::process::Command;

    let output = Command::new("reg")
        .args(["query", key, "/ve"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("REG_SZ") {
            let path_str = line.split("REG_SZ").last()?.trim();
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
    }
    None
}

/// Query a named registry value via `reg.exe`.
#[cfg(target_os = "windows")]
fn registry_named_value(key: &str, name: &str) -> Option<PathBuf> {
    use std::process::Command;

    let output = Command::new("reg")
        .args(["query", key, "/v", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.trim_start().starts_with(name) && line.contains("REG_SZ") {
            let path_str = line.split("REG_SZ").last()?.trim();
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
    }
    None
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod resolve_binary_tests {
    use super::*;

    /// A configured value that is unlikely to ever be a real binary on this
    /// machine's PATH, so `resolves_on_path` deterministically returns
    /// false without the test having to mutate the process-wide PATH env
    /// var (see D145: an env-mutating test that doesn't guard the whole
    /// var turns one red into three).
    const UNRESOLVABLE_BARE_NAME: &str = "konnect-test-fixture-kicad-cli.exe";

    fn make_fake_install(root: &std::path::Path, version: &str) -> PathBuf {
        let bin_dir = root
            .join("Programs")
            .join("KiCad")
            .join(version)
            .join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let cli = bin_dir.join(UNRESOLVABLE_BARE_NAME);
        fs::write(&cli, b"fake kicad-cli").unwrap();
        cli
    }

    // The observable this diagnosis is about: `kicad_cli` unconfigured
    // (bare name) and nothing on PATH -> a per-user install prefix
    // (%LOCALAPPDATA%\Programs\KiCad\<ver>\bin\) is still found.
    #[test]
    fn per_user_install_prefix_is_found_when_not_on_path() {
        let tmp = tempfile::tempdir().unwrap();
        let expected = make_fake_install(tmp.path(), "10.0");

        let standard_paths = kicad_standard_paths(UNRESOLVABLE_BARE_NAME, Some(tmp.path()));
        let (path, source) = resolve_binary(
            UNRESOLVABLE_BARE_NAME,
            UNRESOLVABLE_BARE_NAME,
            &standard_paths,
            || None,
        );

        assert_eq!(source, KicadCliSource::StandardPath);
        assert_eq!(path, expected);
    }

    #[test]
    fn no_install_found_anywhere_keeps_configured_value_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        // Nothing written under `tmp` — no fake install.
        let standard_paths = kicad_standard_paths(UNRESOLVABLE_BARE_NAME, Some(tmp.path()));
        let (path, source) = resolve_binary(
            UNRESOLVABLE_BARE_NAME,
            UNRESOLVABLE_BARE_NAME,
            &standard_paths,
            || None,
        );

        // Bruyant: rien n'est trouvé, la valeur configurée (le nom nu) doit
        // survivre telle quelle pour que l'erreur de spawn reste claire.
        assert_eq!(source, KicadCliSource::Configured);
        assert_eq!(path, PathBuf::from(UNRESOLVABLE_BARE_NAME));
    }

    #[test]
    fn explicit_configured_path_always_wins_even_if_other_candidates_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let _other_install = make_fake_install(tmp.path(), "10.0");

        let explicit = tmp.path().join("my-own-kicad-cli.exe");
        let configured = explicit.to_string_lossy().to_string();

        let standard_paths = kicad_standard_paths(UNRESOLVABLE_BARE_NAME, Some(tmp.path()));
        let (path, source) =
            resolve_binary(&configured, UNRESOLVABLE_BARE_NAME, &standard_paths, || {
                None
            });

        assert_eq!(source, KicadCliSource::Configured);
        assert_eq!(path, explicit);
    }

    /// A bare name that is neither the platform default nor found anywhere
    /// is still explicit — it must not be silently swapped for whatever the
    /// registry happens to find (this pins the bug the first version of this
    /// change had: any unresolvable bare name fell through to the registry
    /// and picked up an unrelated real install).
    #[test]
    fn bare_name_other_than_the_default_is_never_replaced_by_the_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_hit = tmp.path().join("registry-found-kicad-cli.exe");
        fs::write(&registry_hit, b"fake").unwrap();

        let standard_paths = kicad_standard_paths("kicad-cli.exe", Some(tmp.path()));
        let hit_clone = registry_hit.clone();
        let (path, source) = resolve_binary(
            UNRESOLVABLE_BARE_NAME,
            "kicad-cli.exe",
            &standard_paths,
            move || Some(hit_clone),
        );

        assert_eq!(source, KicadCliSource::Configured);
        assert_eq!(path, PathBuf::from(UNRESOLVABLE_BARE_NAME));
    }

    #[test]
    fn registry_probe_is_tried_when_standard_paths_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_hit = tmp.path().join("registry-found-kicad-cli.exe");
        fs::write(&registry_hit, b"fake").unwrap();

        let standard_paths: Vec<PathBuf> = vec![]; // force a miss
        let hit_clone = registry_hit.clone();
        let (path, source) = resolve_binary(
            UNRESOLVABLE_BARE_NAME,
            UNRESOLVABLE_BARE_NAME,
            &standard_paths,
            move || Some(hit_clone),
        );

        assert_eq!(source, KicadCliSource::Registry);
        assert_eq!(path, registry_hit);
    }

    #[test]
    fn kicad_standard_paths_generalizes_to_the_gui_binary_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let cli_paths = kicad_standard_paths("kicad-cli.exe", Some(tmp.path()));
        let gui_paths = kicad_standard_paths("kicad.exe", Some(tmp.path()));

        assert_eq!(cli_paths.len(), gui_paths.len());
        for (cli, gui) in cli_paths.iter().zip(gui_paths.iter()) {
            assert_eq!(cli.parent(), gui.parent());
            assert_eq!(cli.file_name().unwrap(), "kicad-cli.exe");
            assert_eq!(gui.file_name().unwrap(), "kicad.exe");
        }
    }

    /// A per-user KiCad 10 must be preferred over a system-wide KiCad 9: the
    /// candidate list is ordered by version first, prefix second. Listing every
    /// 10.0 prefix and then every 9.0 prefix — with the per-user one appended
    /// after both — had it the other way round.
    #[cfg(target_os = "windows")]
    #[test]
    fn every_candidate_of_a_newer_version_comes_before_any_older_one() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = kicad_standard_paths("kicad-cli.exe", Some(tmp.path()));

        let version_of = |p: &PathBuf| {
            for component in p.components() {
                let s = component.as_os_str().to_string_lossy().to_string();
                if s == "10.0" || s == "9.0" {
                    return s;
                }
            }
            panic!("candidate carries no version component: {}", p.display());
        };

        let last_ten = paths.iter().rposition(|p| version_of(p) == "10.0");
        let first_nine = paths.iter().position(|p| version_of(p) == "9.0");
        assert!(last_ten.is_some() && first_nine.is_some());
        assert!(
            last_ten.unwrap() < first_nine.unwrap(),
            "a 9.0 candidate is tried before a 10.0 one: {paths:?}"
        );

        // …and the per-user prefix is one of the 10.0 candidates, not an
        // afterthought appended behind every older version.
        let per_user_ten = tmp
            .path()
            .join("Programs")
            .join("KiCad")
            .join("10.0")
            .join("bin")
            .join("kicad-cli.exe");
        let per_user_index = paths.iter().position(|p| *p == per_user_ten);
        assert!(per_user_index.is_some(), "per-user 10.0 prefix is missing");
        assert!(per_user_index.unwrap() < first_nine.unwrap());
    }
}
