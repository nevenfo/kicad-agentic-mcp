//! Corpus check: every layer KiCad's own footprint libraries draw on must be
//! representable in the IPC layer enum.
//!
//! This is the measurement P.6.9.1 rests on, kept as a test rather than as a
//! sentence in a commit message. The defect it guards against is not a wrong
//! mapping — it is a *missing* one: `layer_from_name` used to be a
//! hand-written table of fifteen names, and 915 of the 15,433 installed
//! footprints (5.9%) name a layer it did not know — `Dwgs.User`, `Cmts.User`,
//! `F.Adhes`, `Margin`, `User.2` and every inner copper layer past `In2.Cu` —
//! each of which fell through to the `BL_UNDEFINED` sentinel.
//! KiCAD does not validate a scalar layer field on an incoming item; it
//! indexes its layer bitset with whatever arrives, faults, and takes the
//! session's unsaved board with it. So a name this crate cannot represent must
//! never reach KiCAD, and the only honest oracle for "which names occur" is
//! the installed library set.
//!
//! The scan is textual, deliberately. The question is which layer *names* the
//! corpus contains, not how any footprint is structured, and parsing 15,000
//! files to answer it would cost minutes for no more certainty. The whole
//! corpus is read rather than a sample, because a sample would miss exactly
//! the rare name this is looking for.
//!
//! Like the schematic conformance suite, this SKIPs loudly when no KiCad is
//! installed, and asserts its counts when one is, so a silent zero cannot pass
//! for a clean run (D113).

use konnect_ipc::builders::try_layer_from_name;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Pad layer tokens that are wildcards, expanded by the pad builder rather
/// than mapped through the enum.
const WILDCARDS: &[&str] = &["*.Cu", "*.Mask", "*.Paste", "*.SilkS"];

fn footprint_libraries() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KICAD_FOOTPRINTS") {
        let pb = PathBuf::from(&p);
        assert!(
            pb.exists(),
            "KICAD_FOOTPRINTS points at {p}, which does not exist"
        );
        return Some(pb);
    }
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let mut paths = vec![
            PathBuf::from(r"C:\KiCad\10.0\share\kicad\footprints"),
            PathBuf::from(r"C:\Program Files\KiCad\10.0\share\kicad\footprints"),
        ];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join(r"Programs\KiCad\10.0\share\kicad\footprints"));
        }
        paths
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints",
        )]
    } else {
        vec![
            PathBuf::from("/usr/share/kicad/footprints"),
            PathBuf::from("/usr/local/share/kicad/footprints"),
        ]
    };
    candidates.into_iter().find(|p| p.exists())
}

fn collect_footprints(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("kicad_mod") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Layer names appearing in `(layer "X")` and `(layers "A" "B" …)` nodes.
fn layer_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (tag, opens_a_list) in [("(layer ", false), ("(layers ", true)] {
        let mut rest = source;
        while let Some(at) = rest.find(tag) {
            rest = &rest[at + tag.len()..];
            let Some(close) = rest.find(')') else { break };
            let body = &rest[..close];
            for token in body.split('"').skip(1).step_by(2) {
                names.push(token.to_string());
                if !opens_a_list {
                    break;
                }
            }
        }
    }
    names
}

#[test]
fn every_layer_the_official_footprint_libraries_use_is_representable() {
    let Some(root) = footprint_libraries() else {
        eprintln!("SKIP: no KiCad footprint libraries found (set KICAD_FOOTPRINTS to enable)");
        return;
    };
    let files = collect_footprints(&root);
    assert!(
        files.len() > 1000,
        "only {} footprints found under {} — that is not the official library \
         set, and a near-empty corpus must not read as a pass",
        files.len(),
        root.display()
    );

    // name -> (occurrences, first file that carried it)
    let mut seen: BTreeMap<String, (usize, PathBuf)> = BTreeMap::new();
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for name in layer_names(&source) {
            let entry = seen.entry(name).or_insert((0, file.clone()));
            entry.0 += 1;
        }
    }

    let unrepresentable: BTreeSet<_> = seen
        .iter()
        .filter(|(name, _)| {
            !WILDCARDS.contains(&name.as_str()) && try_layer_from_name(name).is_none()
        })
        .map(|(name, (count, file))| {
            format!(
                "{name} ({count} occurrences, e.g. {})",
                file.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect();

    eprintln!(
        "layer corpus: {} footprints, {} distinct layer names",
        files.len(),
        seen.len()
    );
    assert!(
        seen.len() >= 10,
        "only {} distinct layer names across {} footprints — the scan read \
         nothing useful",
        seen.len(),
        files.len()
    );
    assert!(
        unrepresentable.is_empty(),
        "these layers occur in KiCad's own footprints and have no \
         representation in the IPC enum, so placing a footprint that uses one \
         would send KiCAD a layer it faults on:\n  {}",
        unrepresentable.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}
