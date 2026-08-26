//! Where a symbol's `(instances …)` block says it lives (P.6.9.3).
//!
//! Both halves of that block belong to the ROOT sheet: the project name comes
//! from the `.kicad_pro`, and the path starts at the root's uuid and steps
//! through the uuid of each `(sheet …)` on the way down. Taking either from the
//! file being written into is right on a root sheet and wrong on every child,
//! where it names a project and a path KiCad matches against nothing — so the
//! symbol reads as unannotated while the tool reports success.
//!
//! Oracle: KiCad's own `complex_hierarchy` demo. A symbol in the child
//! `ampli_ht.kicad_sch` is written `(project "complex_hierarchy")` — not
//! `"ampli_ht"` — with paths of the form
//! `/5b9623a5-…/00000000-…4b3a1333`, one per placement of that sheet, because
//! the demo instantiates it twice.

mod harness;

use harness::Harness;
use konnect_core::tools::sheet_instance_context;
use serde_json::json;
use std::path::{Path, PathBuf};

const ROOT_UUID: &str = "00000000-0000-4000-8000-000000000001";
const SHEET_A_UUID: &str = "00000000-0000-4000-8000-0000000000a1";
const SHEET_B_UUID: &str = "00000000-0000-4000-8000-0000000000b2";

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("the file is writable");
    path
}

/// One `(sheet …)` block pointing at `file`, with `uuid` as its own.
fn sheet_block(uuid: &str, name: &str, file: &str) -> String {
    format!(
        "\t(sheet\n\t\t(at 40 50)\n\t\t(size 80 25)\n\t\t(uuid \"{uuid}\")\n\
         \t\t(property \"Sheetname\" \"{name}\"\n\t\t\t(at 40 49.365 0)\n\t\t)\n\
         \t\t(property \"Sheetfile\" \"{file}\"\n\t\t\t(at 40 75.635 0)\n\t\t)\n\t)\n"
    )
}

/// A root schematic carrying `sheets`, beside a `.kicad_pro` of the same stem.
fn project_root(dir: &Path, stem: &str, sheets: &str) -> PathBuf {
    write(dir, &format!("{stem}.kicad_pro"), "{}");
    write(
        dir,
        &format!("{stem}.kicad_sch"),
        &format!(
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"konnect\")\n\
             \t(generator_version \"10.0\")\n\t(uuid \"{ROOT_UUID}\")\n\t(paper \"A4\")\n\
             {sheets}\t(sheet_instances\n\t\t(path \"/\" (page \"1\"))\n\t)\n)\n"
        ),
    )
}

fn blank_child(dir: &Path, name: &str) -> PathBuf {
    write(dir, name, &konnect_core::tools::blank_schematic_template())
}

/// The same child, but carrying `Device:R` in its own `lib_symbols`.
///
/// P.7.1: the end-to-end test below places a component, and
/// `library::ensure_lib_symbol` answers a `lib_id` from the *installed*
/// libraries when the schematic does not already embed it. On the machine
/// this was written on KiCad 10 is installed, so `Device:R` resolved and the
/// test passed; on CI, where nothing installs KiCad for the unit job, the
/// tool refused and wrote nothing, and the assertion below then read a file
/// the tool had never touched. Embedding the symbol makes the placement a
/// property of the fixture, the way `harness::TWO_RESISTORS` already documents
/// for every other test that places one.
fn child_with_device_r(dir: &Path, name: &str) -> PathBuf {
    let src = harness::fixtures_dir().join(harness::TWO_RESISTORS);
    let body = std::fs::read_to_string(&src).expect("the fixture is readable");
    write(dir, name, &body)
}

/// The defect, at the level the derivation happens: the project is the one the
/// `.kicad_pro` names, and the path starts at the root, stepping through the
/// `(sheet …)` that placed this file.
#[test]
fn a_child_sheet_is_keyed_to_the_root_and_not_to_itself() {
    let h = Harness::new();
    let dir = h.path("");
    project_root(
        &dir,
        "proj",
        &sheet_block(SHEET_A_UUID, "Amp", "child.kicad_sch"),
    );
    let child = blank_child(&dir, "child.kicad_sch");

    let ctx = sheet_instance_context(&child).expect("the child belongs to proj");
    assert_eq!(
        ctx.project_name, "proj",
        "the project is the .kicad_pro's, not the sheet file's stem"
    );
    assert_eq!(ctx.paths, vec![format!("/{ROOT_UUID}/{SHEET_A_UUID}")]);
}

/// KiCad's `complex_hierarchy` places `ampli_ht.kicad_sch` twice, and its
/// symbols carry one `(path …)` per placement. A symbol written with only one
/// of them is annotated in one instance and invisible in the other.
#[test]
fn a_sheet_placed_twice_gets_one_path_per_placement() {
    let h = Harness::new();
    let dir = h.path("");
    let sheets = format!(
        "{}{}",
        sheet_block(SHEET_A_UUID, "Left", "child.kicad_sch"),
        sheet_block(SHEET_B_UUID, "Right", "child.kicad_sch")
    );
    project_root(&dir, "proj", &sheets);
    let child = blank_child(&dir, "child.kicad_sch");

    let ctx = sheet_instance_context(&child).expect("the child belongs to proj");
    let mut expected = vec![
        format!("/{ROOT_UUID}/{SHEET_A_UUID}"),
        format!("/{ROOT_UUID}/{SHEET_B_UUID}"),
    ];
    expected.sort();
    assert_eq!(ctx.paths, expected);
}

/// The fallback the whole change rests on: a root sheet, a schematic in no
/// project, and a neighbour that appears in no sheet tree all keep today's
/// standalone derivation. Resolving those would be a guess.
#[test]
fn everything_unresolvable_falls_back_to_the_standalone_derivation() {
    let h = Harness::new();
    let dir = h.path("");
    let root = project_root(
        &dir,
        "proj",
        &sheet_block(SHEET_A_UUID, "Amp", "child.kicad_sch"),
    );
    blank_child(&dir, "child.kicad_sch");
    let stranger = blank_child(&dir, "stranger.kicad_sch");

    assert!(
        sheet_instance_context(&root).is_none(),
        "a root sheet is already keyed to itself, correctly"
    );
    assert!(
        sheet_instance_context(&stranger).is_none(),
        "a file in no sheet tree is not placed by anything"
    );

    let loose_dir = h.path("loose");
    std::fs::create_dir_all(&loose_dir).expect("the directory is creatable");
    let loose = blank_child(&loose_dir, "loose.kicad_sch");
    assert!(
        sheet_instance_context(&loose).is_none(),
        "a schematic belonging to no project keeps standalone behaviour"
    );
}

/// End to end through the real tool: the block that reaches the file is the
/// one KiCad reads the designator from.
#[tokio::test]
async fn a_component_placed_on_a_child_sheet_is_written_with_the_roots_path() {
    let h = Harness::new();
    let dir = h.path("");
    project_root(
        &dir,
        "proj",
        &sheet_block(SHEET_A_UUID, "Amp", "child.kicad_sch"),
    );
    let child = child_with_device_r(&dir, "child.kicad_sch");

    h.json(
        "add_schematic_component",
        json!({
            "schematic": harness::as_str(&child),
            "lib_id": "Device:R",
            "reference": "R201",
            "value": "10k",
            "x": 50.8,
            "y": 50.8
        }),
    )
    .await;

    let text = std::fs::read_to_string(&child).expect("the child is readable");
    assert!(
        text.contains(r#"(project "proj""#),
        "the instance names the sheet's own stem, not its project: {text}"
    );
    assert!(
        text.contains(&format!(r#"(path "/{ROOT_UUID}/{SHEET_A_UUID}""#)),
        "the instance path must start at the root and step through the sheet"
    );
}
