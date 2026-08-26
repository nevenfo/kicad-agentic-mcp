//! Live probe for schematic fidelity (#144, #209): does KiCAD accept what this
//! engine writes for a custom `(paper …)` size, and does an unrelated edit to a
//! `(lib_name …)`-derived schematic leave every net exactly as it was?
//!
//! The unit tests in `konnect-schematic-editor` and `konnect-core` prove the
//! syntax against the rules as written down. Only KiCAD can say whether those
//! rules are its own — and this project has been bitten before by files that
//! looked right and did not load, or loaded and silently rewired. So this
//! builds real schematics with the engine and asks `kicad-cli`:
//!
//! 1. does a custom-page schematic export a netlist at all, and
//! 2. does editing one derived symbol's Value change the netlist's nets in any
//!    way other than that Value?
//!
//! `#[ignore]`d because it needs a real `kicad-cli`. Run with:
//!
//!     KICAD_CLI=<path> cargo test -p konnect-core --test schematic_fidelity_live -- --ignored --nocapture

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use konnect_schematic_editor as cse;

fn kicad_cli() -> String {
    if let Ok(path) = std::env::var("KICAD_CLI") {
        assert!(
            Path::new(&path).exists(),
            "KICAD_CLI points at {path}, which does not exist"
        );
        return path;
    }
    let candidates: &[&str] = if cfg!(windows) {
        &[
            r"C:\Program Files\KiCad\10.0\bin\kicad-cli.exe",
            r"C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"]
    } else {
        &["/usr/bin/kicad-cli", "/usr/local/bin/kicad-cli"]
    };
    candidates
        .iter()
        .find(|path| Path::new(path).exists())
        .map(|path| path.to_string())
        .unwrap_or_else(|| panic!("no kicad-cli found — set KICAD_CLI to run this probe"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn run(cli: &str, args: &[&str]) -> (bool, String) {
    let output = Command::new(cli)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("kicad-cli failed to start: {e}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

fn export_netlist(cli: &str, sch: &Path, out: &Path) -> String {
    let (ok, output) = run(
        cli,
        &[
            "sch",
            "export",
            "netlist",
            "--output",
            out.to_str().unwrap(),
            sch.to_str().unwrap(),
        ],
    );
    assert!(ok, "netlist export failed:\n{output}");
    std::fs::read_to_string(out).expect("the netlist is readable")
}

/// The nets a KiCAD netlist declares, keyed by net name, each value being the
/// set of `(reference, pin)` pairs attached to it. Ignores net `code` (a
/// sequence number that can shift between exports) and any tstamp/uuid/date
/// fields, comparing only the connectivity a schematic edit could disturb.
fn net_membership(netlist: &str) -> BTreeMap<String, BTreeSet<(String, String)>> {
    let tree = konnect_sexp::parser::parse_sexp(netlist).expect("netlist parses as valid sexp");
    let nets = tree
        .find("nets")
        .map(|n| n.find_all("net"))
        .unwrap_or_default();

    let mut out = BTreeMap::new();
    for net in nets {
        let name = net
            .find_str("name")
            .unwrap_or_else(|| panic!("net has no name: {net:?}"))
            .to_string();
        let members: BTreeSet<(String, String)> = net
            .find_all("node")
            .iter()
            .map(|node| {
                let ref_ = node.find_str("ref").unwrap_or("").to_string();
                let pin = node.find_str("pin").unwrap_or("").to_string();
                (ref_, pin)
            })
            .collect();
        out.insert(name, members);
    }
    out
}

/// A schematic containing a lone resistor on a custom `(paper "User" …)` page,
/// built through the engine's own API rather than hand-written source — this
/// is the shape `add_schematic_component` and friends actually produce.
fn build_custom_paper_schematic(dir: &Path) -> PathBuf {
    let path = dir.join("custom_paper.kicad_sch");
    std::fs::write(
        &path,
        "(kicad_sch\n\t(version 20250316)\n\t(generator \"konnect-schematic-fidelity-probe\")\n\t(uuid \"f0000000-0000-4000-8000-000000000201\")\n\t(paper \"A4\")\n\t(lib_symbols\n\t\t(symbol \"Device:R\"\n\t\t\t(pin_names (offset 0))\n\t\t\t(pin_numbers (hide yes))\n\t\t\t(exclude_from_sim no)\n\t\t\t(in_bom yes)\n\t\t\t(on_board yes)\n\t\t\t(property \"Reference\" \"R\" (at 2.032 0 90))\n\t\t\t(property \"Value\" \"R\" (at 0 0 90))\n\t\t\t(symbol \"R_0_1\"\n\t\t\t\t(pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\"))\n\t\t\t\t(pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\") (number \"2\"))\n\t\t\t)\n\t\t)\n\t)\n\t(symbol\n\t\t(lib_id \"Device:R\")\n\t\t(at 101.6 50.8 0)\n\t\t(unit 1)\n\t\t(exclude_from_sim no)\n\t\t(in_bom yes)\n\t\t(on_board yes)\n\t\t(uuid \"f0000000-0000-4000-8000-000000000211\")\n\t\t(property \"Reference\" \"R1\" (at 103.632 50.8 90))\n\t\t(property \"Value\" \"10k\" (at 101.6 50.8 90))\n\t\t(instances (project \"\" (path \"/\" (reference \"R1\") (unit 1))))\n\t)\n)\n",
    )
    .expect("write probe fixture");

    let mut sch = cse::Schematic::load(&path).expect("the probe fixture loads");
    // `User` requires its width and height (#209) — the whole point of this
    // probe is that a bare `(paper "User")` is what KiCAD used to receive.
    sch.paper = Some("User".to_string());
    sch.paper_args = vec![
        cse::sexp::atom("292.1".to_string()),
        cse::sexp::atom("205.105".to_string()),
    ];
    sch.overwrite().expect("the schematic is writable");
    path
}

/// KiCAD accepts a custom `(paper "User" 292.1 205.105)` page this engine
/// wrote, and exports a netlist from it — the bar #209 fixed: a `(paper
/// "User")` with no dimensions made KiCAD refuse to load the file at all.
#[test]
#[ignore = "requires kicad-cli; run with --ignored"]
fn kicad_accepts_a_custom_paper_size_this_engine_wrote() {
    let cli = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_custom_paper_schematic(dir.path());
    let netlist_path = dir.path().join("custom_paper.net");

    let (ok, output) = run(
        &cli,
        &[
            "sch",
            "export",
            "netlist",
            "--output",
            netlist_path.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
    assert!(
        ok,
        "kicad-cli refused the custom-paper schematic this engine wrote:\n{output}"
    );
    assert!(
        netlist_path.exists(),
        "no netlist was written (exit ok = {ok}):\n{output}"
    );
}

/// An edit to one derived (`lib_name`) symbol's Value must not move, merge, or
/// drop any net — including nets belonging to symbols the edit never touched
/// (#143's reported corruption: a single edit re-serialized the whole file and
/// dropped `lib_name` from every other symbol, re-pointing them at the wrong
/// library definition and silently rewiring the netlist).
#[test]
#[ignore = "requires kicad-cli; run with --ignored"]
fn editing_a_derived_symbol_does_not_disturb_any_net() {
    let cli = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("derived_lib_name.kicad_sch");
    std::fs::copy(fixtures_dir().join("derived_lib_name.kicad_sch"), &path)
        .expect("the fixture is copyable");

    let before_net = dir.path().join("before.net");
    let before_text = export_netlist(&cli, &path, &before_net);
    let before = net_membership(&before_text);
    assert!(!before.is_empty(), "fixture must export at least one net");

    // The unrelated edit: R1's Value, via the schematic editor — the same
    // path `konnect_schematic_editor` tools use.
    let mut sch = cse::Schematic::load(&path).expect("the fixture loads");
    sch.symbols
        .by_reference_mut("R1")
        .expect("R1 is in the fixture")
        .set_value_str("4.7k");
    sch.overwrite().expect("the schematic is writable");

    let after_net = dir.path().join("after.net");
    let after_text = export_netlist(&cli, &path, &after_net);
    let after = net_membership(&after_text);

    assert_eq!(
        before, after,
        "editing R1.Value must not change any net's membership:\nbefore:\n{before:#?}\nafter:\n{after:#?}"
    );
}
