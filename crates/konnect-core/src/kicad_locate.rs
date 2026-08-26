//! Shared KiCAD binary-location logic (D149), used by both the MCP server
//! (to resolve `kicad_cli`/`kicad_binary` for spawning tools) and the
//! `konnect` installer (to auto-detect KiCAD at `init`/`status` time).
//!
//! Lives in `konnect-core` — not the `konnect` crate — because `konnect`
//! depends on `konnect-core`, not the other way round; putting it here lets
//! both consumers share the exact same resolution order without an inverted
//! dependency.

use std::path::{Path, PathBuf};

/// Where a resolved KiCAD binary path came from — logged once at startup so a
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

/// Resolve a KiCAD binary (`kicad-cli`/`kicad`) against the machine, in
/// priority order:
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

/// Query the Windows registry for a KiCAD install location:
/// - `HKLM\SOFTWARE\KiCad\<ver>` (default value, `/ve`) — set by an
///   admin/machine-wide install;
/// - the per-user uninstall key (`InstallLocation`), HKCU then HKLM — what a
///   per-user install (the installer run without admin rights) actually
///   writes.
///
/// `binary_filename` is `kicad-cli.exe` or `kicad.exe` depending on caller.
#[cfg(target_os = "windows")]
pub fn detect_kicad_from_registry(binary_filename: &str) -> Option<PathBuf> {
    if let Some(install_dir) = registry_default_value(r"HKLM\SOFTWARE\KiCad\10.0") {
        let bin_path = install_dir.join("bin").join(binary_filename);
        if bin_path.exists() {
            return Some(bin_path);
        }
    }

    for hive in ["HKCU", "HKLM"] {
        let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\KiCad 10.0");
        if let Some(install_dir) = registry_named_value(&key, "InstallLocation") {
            let bin_path = install_dir.join("bin").join(binary_filename);
            if bin_path.exists() {
                return Some(bin_path);
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
    use std::fs;

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
    #[cfg(target_os = "windows")]
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

    /// The observable this fix (F-16) is about: `find_kicad_binary` (in
    /// `konnect-core::tools::verification`) uses this same resolver for the
    /// GUI binary, so a per-user KiCad 10 install must be found for
    /// `kicad.exe` too, not just `kicad-cli.exe`.
    #[cfg(target_os = "windows")]
    #[test]
    fn per_user_install_prefix_is_found_for_the_gui_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp
            .path()
            .join("Programs")
            .join("KiCad")
            .join("10.0")
            .join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let gui = bin_dir.join(UNRESOLVABLE_BARE_NAME);
        fs::write(&gui, b"fake kicad").unwrap();

        let standard_paths = kicad_standard_paths(UNRESOLVABLE_BARE_NAME, Some(tmp.path()));
        let (path, source) = resolve_binary(
            UNRESOLVABLE_BARE_NAME,
            UNRESOLVABLE_BARE_NAME,
            &standard_paths,
            || None,
        );

        assert_eq!(source, KicadCliSource::StandardPath);
        assert_eq!(path, gui);
    }
}
