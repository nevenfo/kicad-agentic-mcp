//! Live probe for buses (J.2.2.2): does KiCAD read back what this engine
//! writes, and does its connectivity derive the members `expand_members` says?
//!
//! The unit tests prove the syntax against the rules as written down. Only
//! KiCAD can say whether those rules are its own — and the project has been
//! bitten before by a file that looked right and did not load (the quoted
//! `generator` field). So this builds a real bus schematic with the engine and
//! asks `kicad-cli` two questions:
//!
//! 1. does it load the file at all, and
//! 2. what does it call the nets?
//!
//! The second is the one that matters: the netlist names come from KiCAD's own
//! bus expansion, so a mismatch means this repository's expansion is wrong, not
//! that the test is stale.
//!
//! `#[ignore]`d because it needs a real `kicad-cli`. The fixture embeds its
//! `Device:R` symbol, so no installed libraries are needed. Run with:
//!
//!     KICAD_CLI=<path> cargo test -p konnect-core --test bus_live -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;

use konnect_schematic_editor as cse;

/// The bus label the probe writes. Four members, two of them wired to a pin.
const BUS_LABEL: &str = "DATA[0..3]";

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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bus_two_resistors.kicad_sch")
}

/// Build the probe schematic: a bus labelled `DATA[0..3]`, two bus entries, and
/// a wire from each entry to a resistor pin, labelled `DATA0` and `DATA1`.
///
/// Coordinates are on KiCAD's 1.27 mm schematic grid and the wires end exactly
/// on the pins the fixture places, because connectivity is by coincidence of
/// points and nothing else.
fn build_bus_schematic(dir: &Path) -> PathBuf {
    let path = dir.join("bus_probe.kicad_sch");
    std::fs::copy(fixture(), &path).expect("the fixture is copyable");

    let mut sch = cse::Schematic::load(&path).expect("the fixture loads");

    sch.add_bus(96.52, 38.1, 127.0, 38.1);
    sch.add_label(BUS_LABEL, 99.06, 38.1);

    // R1's pin 1 is at (101.6, 46.99); R2's at (114.3, 46.99).
    for (entry_x, wire_x, member) in [(104.14, 101.6, "DATA0"), (116.84, 114.3, "DATA1")] {
        sch.add_bus_entry(entry_x, 38.1, -2.54, 2.54);
        sch.add_wire(wire_x, 40.64, wire_x, 46.99);
        sch.add_label(member, wire_x, 43.18);
    }

    sch.overwrite().expect("the schematic is writable");
    path
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

/// KiCAD loads a schematic this engine wrote with buses in it. The bar is low
/// and it is the one the project has actually tripped over.
#[test]
#[ignore = "requires kicad-cli; run with --ignored"]
fn kicad_loads_a_schematic_this_engine_wrote_with_buses() {
    let cli = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bus_schematic(dir.path());
    let erc = dir.path().join("erc.json");

    let (ok, output) = run(
        &cli,
        &[
            "sch",
            "erc",
            "--format",
            "json",
            "--output",
            erc.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
    assert!(
        !output.to_lowercase().contains("failed to load"),
        "kicad-cli could not load the file this engine wrote:\n{output}"
    );
    // ERC exits non-zero when it finds violations, which is not what is being
    // asserted here — only that the document parsed and was checked.
    assert!(
        erc.exists(),
        "ERC produced no report (exit ok = {ok}):\n{output}"
    );
}

/// The claim `expand_members` makes about vector syntax, checked against
/// KiCAD's own connectivity: the netlist names the members, not the bus.
#[test]
#[ignore = "requires kicad-cli; run with --ignored"]
fn kicad_names_the_nets_the_way_expand_members_does() {
    let cli = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = build_bus_schematic(dir.path());
    let netlist = dir.path().join("probe.net");

    let (ok, output) = run(
        &cli,
        &[
            "sch",
            "export",
            "netlist",
            "--output",
            netlist.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
    assert!(ok, "netlist export failed:\n{output}");

    let text = std::fs::read_to_string(&netlist).expect("the netlist is readable");
    let (kind, members) =
        cse::schematic::bus::expand_members(BUS_LABEL, &[]);
    assert_eq!(kind, cse::schematic::bus::BusKind::Vector);

    // The two members with a pin on them must appear as nets by exactly the
    // names this repository derives. KiCAD prefixes a local net with its sheet
    // path, so the root sheet's `DATA0` is written `/DATA0` — the prefix is the
    // sheet, the rest is the member name and that is what is being checked.
    for member in &members[..2] {
        assert!(
            text.contains(&format!("(name \"/{member}\")")),
            "KiCAD's netlist has no net called '/{member}' — this repository's \
             bus expansion disagrees with KiCAD's.\nNetlist:\n{text}"
        );
    }
    // And the bus name itself is not a net: it stands for its members.
    assert!(
        !text.contains(BUS_LABEL),
        "'{BUS_LABEL}' appears in the netlist; KiCAD did not expand the bus.\nNetlist:\n{text}"
    );
}
