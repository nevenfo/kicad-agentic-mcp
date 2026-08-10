//! Where KiCad keeps its bundled libraries.
//!
//! This lives in one place on purpose. The discovery list used to be written
//! twice — once in `konnect-core::tools::kicad_share_roots` for symbols,
//! footprints and 3D models, and once in `library::find_symbol_dirs` for
//! symbol lookups — and the two copies had already drifted: the core list knew
//! about KiCad 8, the library list stopped at 9, and *neither* knew about
//! per-user Windows installs. A machine where `winget install KiCad.KiCad`
//! produced "KiCad 10.0 (current user)" under `%LOCALAPPDATA%\Programs` failed
//! every `lib_id` resolution with `Library 'Device' not found`, unless KiCad
//! itself had launched the server and exported `KICAD10_SYMBOL_DIR`.
//!
//! Two entry points, one list:
//!   * [`share_roots`] — directories that directly contain `symbols/`,
//!     `footprints/`, `3dmodels/`.
//!   * [`library_dirs`] — a specific library kind, environment overrides first.

use std::path::PathBuf;

/// KiCad majors we look for, newest first. A newer major must always outrank
/// an older one *across all prefixes*, otherwise a stale `C:\KiCad\8.0` shadows
/// a current per-user 10.0 install.
const MAJORS: [&str; 3] = ["10", "9", "8"];

/// The `KICAD<major>_…` environment-variable suffix naming a library kind.
/// `None` means the kind has no environment override.
pub fn env_suffix(kind: &str) -> Option<&'static str> {
    match kind {
        "symbols" => Some("SYMBOL_DIR"),
        "footprints" => Some("FOOTPRINT_DIR"),
        "3dmodels" => Some("3DMODEL_DIR"),
        _ => None,
    }
}

/// Roots under which KiCad ships its bundled libraries.
///
/// Existence is *not* filtered here — callers that need real directories use
/// [`library_dirs`], which filters. Returning the raw candidate list keeps the
/// ordering testable on machines that have no KiCad installed at all.
pub fn share_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // `%ProgramFiles%` comes from the environment rather than a hard-coded
        // `C:\Program Files` so a non-C: or localized prefix still resolves.
        let mut prefixes: Vec<PathBuf> = vec![PathBuf::from(r"C:\")];
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(p) = std::env::var_os(var) {
                prefixes.push(PathBuf::from(p));
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            prefixes.push(PathBuf::from(local).join("Programs"));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            prefixes.push(PathBuf::from(appdata));
        }

        for major in MAJORS {
            for prefix in &prefixes {
                roots.push(
                    prefix
                        .join("KiCad")
                        .join(format!("{major}.0"))
                        .join("share")
                        .join("kicad"),
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from(
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport",
        ));
        if let Ok(home) = std::env::var("HOME") {
            // Per-user install (KiCad.app dragged into ~/Applications).
            roots.push(
                PathBuf::from(home).join("Applications/KiCad/KiCad.app/Contents/SharedSupport"),
            );
        }
        roots.push(PathBuf::from("/opt/homebrew/share/kicad"));
        roots.push(PathBuf::from("/usr/local/share/kicad"));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        roots.push(PathBuf::from("/usr/share/kicad"));
        roots.push(PathBuf::from("/usr/local/share/kicad"));
        roots.push(PathBuf::from("/opt/kicad/share/kicad"));
        roots.push(PathBuf::from(
            "/var/lib/flatpak/app/org.kicad.KiCad/current/active/files/share/kicad",
        ));
        if let Ok(home) = std::env::var("HOME") {
            roots.push(
                PathBuf::from(home).join(
                    ".local/share/flatpak/app/org.kicad.KiCad/current/active/files/share/kicad",
                ),
            );
        }
        roots.push(PathBuf::from("/snap/kicad/current/usr/share/kicad"));
    }

    roots
}

/// Existing directories holding libraries of `kind` (`"symbols"`,
/// `"footprints"`, `"3dmodels"`), most-preferred first.
///
/// `KICAD<major>_<KIND>_DIR` wins over the bundled roots: when KiCad launches
/// an add-on it exports those variables, and they are the only correct answer
/// on a relocated or portable install.
pub fn library_dirs(kind: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() && !dirs.contains(&p) {
            dirs.push(p);
        }
    };

    if let Some(suffix) = env_suffix(kind) {
        for major in MAJORS {
            // `var_os`, not `var`: a directory whose name is not valid Unicode
            // is still a directory KiCad may have pointed us at, and `var`
            // reports those as absent.
            if let Some(dir) = std::env::var_os(format!("KICAD{major}_{suffix}")) {
                push(PathBuf::from(dir));
            }
        }
    }

    for root in share_roots() {
        push(root.join(kind));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_roots_is_not_empty() {
        assert!(!share_roots().is_empty());
    }

    #[test]
    fn env_suffix_covers_the_three_library_kinds() {
        assert_eq!(env_suffix("symbols"), Some("SYMBOL_DIR"));
        assert_eq!(env_suffix("footprints"), Some("FOOTPRINT_DIR"));
        assert_eq!(env_suffix("3dmodels"), Some("3DMODEL_DIR"));
        assert_eq!(env_suffix("templates"), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_roots_include_the_per_user_winget_prefix() {
        // `winget install KiCad.KiCad` installs "KiCad 10.0 (current user)"
        // here. Missing it made every symbol lookup fail on a stock install.
        let local = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is set on Windows");
        let expected = PathBuf::from(local)
            .join("Programs")
            .join("KiCad")
            .join("10.0")
            .join("share")
            .join("kicad");
        assert!(
            share_roots().contains(&expected),
            "per-user install prefix missing from share_roots(): {expected:?}"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn newer_majors_outrank_older_ones_across_every_prefix() {
        let roots = share_roots();
        let last_10 = roots
            .iter()
            .rposition(|p| p.to_string_lossy().contains("10.0"))
            .expect("a 10.0 candidate must exist");
        let first_9 = roots
            .iter()
            .position(|p| p.to_string_lossy().contains("9.0"))
            .expect("a 9.0 candidate must exist");
        assert!(
            last_10 < first_9,
            "a KiCad 9 path is searched before a KiCad 10 path; a stale old install would win"
        );
    }
}
