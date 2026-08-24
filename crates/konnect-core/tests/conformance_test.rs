//! Golden-file conformance suite.
//!
//! Oracle: schematics authored by eeschema itself. KiCAD installs a demo
//! corpus (`share/kicad/demos`) full of real, hierarchy-heavy, multi-unit
//! designs — if our parser or editors disagree with anything in there, we
//! disagree with KiCAD.
//!
//! These tests locate an installed KiCAD (or `KICAD_DEMOS` env override) and
//! SKIP silently when none is present, so plain CI stays green while the
//! scheduled real-KiCAD workflow and local dev runs get full coverage.
//! (Same skip pattern the predecessor project used for its kicad-cli tests.)

use konnect_sexp::{parse_sexp, writer};
use std::path::PathBuf;

fn demo_dirs() -> Option<PathBuf> {
    // An explicit KICAD_DEMOS that does not exist is a mistake worth shouting
    // about: the whole point of setting it is to stop these tests skipping,
    // and a typo would otherwise skip them just as quietly as no KiCAD at all.
    if let Ok(p) = std::env::var("KICAD_DEMOS") {
        let pb = PathBuf::from(&p);
        assert!(
            pb.exists(),
            "KICAD_DEMOS points at {p}, which does not exist"
        );
        return Some(pb);
    }
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        // A per-user install lives under %LOCALAPPDATA%, and leaving it out is
        // why these tests reported "passed" in 0.00 s on a machine that did
        // have KiCad: the lookup simply never found the corpus.
        let mut paths = vec![
            PathBuf::from(r"C:\KiCad\10.0\share\kicad\demos"),
            PathBuf::from(r"C:\Program Files\KiCad\10.0\share\kicad\demos"),
        ];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join(r"Programs\KiCad\10.0\share\kicad\demos"));
        }
        paths
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/demos",
        )]
    } else {
        vec![
            PathBuf::from("/usr/share/kicad/demos"),
            PathBuf::from("/usr/local/share/kicad/demos"),
        ]
    };
    candidates.into_iter().find(|p| p.exists())
}

fn collect_schematics(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "kicad_sch") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Every schematic eeschema ships must parse. This is the broadest format-
/// coverage test we have: hierarchical sheets, multi-unit symbols, buses,
/// text boxes, images — whatever the demos contain, the parser must accept.
#[test]
fn every_installed_demo_schematic_parses() {
    let Some(root) = demo_dirs() else {
        eprintln!("SKIP: no KiCAD demos found (set KICAD_DEMOS to enable)");
        return;
    };
    let schematics = collect_schematics(&root);
    assert!(
        !schematics.is_empty(),
        "demo dir exists but contains no .kicad_sch files: {}",
        root.display()
    );

    let mut parsed = 0usize;
    let mut failures = Vec::new();
    for sch in &schematics {
        let content = std::fs::read_to_string(sch).unwrap_or_default();
        match parse_sexp(&content) {
            Ok(node) => {
                assert_eq!(
                    node.head(),
                    Some("kicad_sch"),
                    "unexpected root in {}",
                    sch.display()
                );
                parsed += 1;
            }
            Err(e) => failures.push(format!("{}: {}", sch.display(), e)),
        }
    }
    eprintln!("parsed {}/{} demo schematics", parsed, schematics.len());
    assert!(
        failures.is_empty(),
        "parser rejected eeschema-authored files:\n  {}",
        failures.join("\n  ")
    );
}

fn collect_boards(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Boards KiCad itself ships that are genuinely malformed, and why.
///
/// This is an allow-list of *measured* facts, not a way to quiet a failing
/// parser: each entry is a file whose own bytes are unbalanced, verified
/// independently of us. Anything else that fails is our bug.
const KNOWN_BAD_BOARDS: &[(&str, &str)] = &[(
    "RoyalBlue54L-Feather.kicad_pcb",
    "3.6 MB whose root closes at byte 14735, ending 349 closing parens ahead; \
     a paren-balance scan over interf_u and pic_programmer returns depth 0, so \
     the imbalance is this file's and not the scanner's",
)];

/// Every board KiCad ships must parse, or be a named, explained exception.
///
/// The board half of the corpus had no conformance test at all, which is how
/// `parse_sexp` could answer `Ok` on a 3.6 MB board while holding three of its
/// pads: nothing ever asked. The counts are printed and asserted so that a run
/// finding zero files fails instead of passing in 0.00 s — the exact trap that
/// made these tests look green on a machine that had KiCad installed all along.
#[test]
fn every_installed_demo_board_parses_or_is_a_known_bad_file() {
    let Some(root) = demo_dirs() else {
        eprintln!("SKIP: no KiCAD demos found (set KICAD_DEMOS to enable)");
        return;
    };
    let boards = collect_boards(&root);
    assert!(
        !boards.is_empty(),
        "demo dir exists but contains no .kicad_pcb files: {}",
        root.display()
    );

    let mut parsed = 0usize;
    let mut failures = Vec::new();
    let mut expected_failures = Vec::new();
    for board in &boards {
        let name = board
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let known_bad = KNOWN_BAD_BOARDS.iter().find(|(file, _)| *file == name);
        let content = std::fs::read_to_string(board).unwrap_or_default();
        match (parse_sexp(&content), known_bad) {
            (Ok(node), None) => {
                assert_eq!(
                    node.head(),
                    Some("kicad_pcb"),
                    "unexpected root in {}",
                    board.display()
                );
                parsed += 1;
            }
            (Err(e), None) => failures.push(format!("{}: {}", board.display(), e)),
            (Err(_), Some((_, why))) => expected_failures.push(format!("{name}: {why}")),
            // A file we recorded as malformed now parses: either KiCad shipped
            // a fixed copy or the parser started accepting damage. Both need a
            // human, so neither may pass silently.
            (Ok(_), Some((_, why))) => failures.push(format!(
                "{} parses, but is on the known-bad list ({why}). Re-measure and \
                 update KNOWN_BAD_BOARDS.",
                board.display()
            )),
        }
    }
    eprintln!(
        "parsed {}/{} demo boards ({} known-bad)",
        parsed,
        boards.len(),
        expected_failures.len()
    );
    for note in &expected_failures {
        eprintln!("  known-bad {note}");
    }
    assert!(
        failures.is_empty(),
        "parser disagreed with pcbnew-authored files:\n  {}",
        failures.join("\n  ")
    );
}

/// Structural extraction must work on real designs: symbols, wires, and
/// labels come back non-empty for the demo corpus as a whole, and pin
/// transforms compute without panicking for every instance.
#[test]
fn demo_corpus_structural_extraction() {
    let Some(root) = demo_dirs() else {
        eprintln!("SKIP: no KiCAD demos found");
        return;
    };
    use konnect_sexp::schematic::{extract_symbol_instances, extract_wires};

    let mut total_symbols = 0usize;
    let mut total_wires = 0usize;
    for sch in collect_schematics(&root) {
        let content = std::fs::read_to_string(&sch).unwrap_or_default();
        let Ok(tree) = parse_sexp(&content) else {
            continue; // parse failures are the previous test's job
        };
        let symbols = extract_symbol_instances(&tree);
        for inst in &symbols {
            // Must never panic, whatever rotation/mirror combination ships.
            let t = inst.pin_transform();
            let _ = konnect_sexp::geometry::transform_pin(1.27, 2.54, t);
        }
        total_symbols += symbols.len();
        total_wires += extract_wires(&tree).len();
    }
    eprintln!("extracted {} symbols, {} wires", total_symbols, total_wires);
    assert!(total_symbols > 100, "suspiciously few symbols extracted");
    assert!(total_wires > 100, "suspiciously few wires extracted");
}

/// Byte-edit safety on real files: applying a no-op edit (insert + delete of
/// the same text) to an eeschema file must leave it byte-identical, and an
/// actual insertion must still re-parse. Guards the predecessor's file-
/// corruption class without needing a full serializer.
#[test]
fn demo_files_survive_edit_cycle() {
    let Some(root) = demo_dirs() else {
        eprintln!("SKIP: no KiCAD demos found");
        return;
    };
    let schematics = collect_schematics(&root);
    // A representative slice keeps this test fast even on huge corpora.
    for sch in schematics.iter().take(10) {
        let original = std::fs::read_to_string(sch).unwrap();

        // No-op: insert marker then delete it again.
        let marker = "(text \"konnect-conformance-probe\")";
        let insert_at = original.rfind(')').unwrap();
        let inserted = writer::apply_edits(
            original.clone(),
            vec![konnect_sexp::SexpEdit {
                start: insert_at,
                end: insert_at,
                replacement: marker.to_string(),
            }],
        );
        assert!(
            parse_sexp(&inserted).is_ok(),
            "insertion broke parseability of {}",
            sch.display()
        );

        let removed = writer::apply_edits(
            inserted.clone(),
            vec![konnect_sexp::SexpEdit {
                start: insert_at,
                end: insert_at + marker.len(),
                replacement: String::new(),
            }],
        );
        assert_eq!(
            removed,
            original,
            "edit round-trip not byte-identical for {}",
            sch.display()
        );
    }
}
