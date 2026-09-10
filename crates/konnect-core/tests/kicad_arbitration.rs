//! The small live corpus: mutations applied to a throwaway project, judged by
//! KiCAD rather than by us (X7).
//!
//! Everything here is `#[ignore]`d and needs a real `kicad-cli` — CI installs
//! none. `scripts/live-pcb-e2e.ps1` and `gate.ps1` are where it runs.
//!
//! It is deliberately small. Its job is not to measure how much of KiCAD this
//! server covers; it is to cover the *classes of failure* the campaign found:
//!
//! * a mutation whose result KiCAD refuses to load at all;
//! * a mutation KiCAD loads and ignores, which reads as success either way;
//! * a refusal that edited the document anyway;
//! * an oracle that is green because it cannot go red.
//!
//! The last one is the one that is easy to forget, and it is why
//! [`the_oracle_can_fail`] exists. Every other test here asserts that a
//! violation *appears*; none of them would notice if `kicad_reloads` had
//! quietly stopped reporting anything at all.

mod harness;

use harness::Harness;
use serde_json::json;

/// The control. Without it, every "KiCAD saw our change" assertion below is
/// unfalsifiable: an oracle that always answers the same thing agrees with
/// anything.
///
/// The fixture leaves 0.75 mm between two tracks on different nets. Below
/// that, KiCAD reports no `clearance` violation; above it, exactly one. The
/// same board, the same call, the same reader — only the value moves.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_oracle_can_fail() {
    let h = Harness::new();
    let board = h.write("control.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("control.kicad_pro", harness::BLANK_PROJECT);
    let board_arg = harness::as_str(&board).to_string();

    let mut seen = Vec::new();
    for mm in [0.2, 1.5, 0.2] {
        h.json(
            "set_design_rules",
            json!({ "board": board_arg, "min_clearance": mm }),
        )
        .await;
        seen.push(harness::kicad_reloads(&board).get("clearance").copied());
    }

    assert_eq!(
        seen,
        vec![None, Some(1), None],
        "the oracle does not track the rule it is supposed to be reading: {seen:?}"
    );
}

/// A board this server has mutated is still a board KiCAD will open.
///
/// This is the cheapest possible guard against the defect class the campaign
/// started from — three tools were writing keys KiCAD does not have — and it
/// costs one `kicad-cli` run per tool.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_board_still_loads_after_each_mutation() {
    let h = Harness::new();
    let board = h.write("mutated.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("mutated.kicad_pro", harness::BLANK_PROJECT);
    let board_arg = harness::as_str(&board).to_string();

    // A read first: it must not change what follows.
    h.json("get_design_rules", json!({ "board": board_arg }))
        .await;
    let baseline = harness::kicad_reloads(&board);

    h.json(
        "add_board_outline",
        json!({ "board": board_arg, "x1": 90.0, "y1": 90.0, "x2": 120.0, "y2": 110.0 }),
    )
    .await;
    harness::kicad_reloads(&board);

    h.json(
        "set_design_rules",
        json!({ "board": board_arg, "min_clearance": 0.2, "min_track_width": 0.15 }),
    )
    .await;
    harness::kicad_reloads(&board);

    h.json(
        "set_layer_constraints",
        json!({ "board": board_arg, "layer": "F.Cu", "min_clearance": 0.2 }),
    )
    .await;
    let after = harness::kicad_reloads(&board);

    // The two tracks were never touched, so the finding that has nothing to do
    // with any of these calls is still exactly what it was.
    assert_eq!(
        baseline.get("track_dangling"),
        after.get("track_dangling"),
        "a mutation disturbed something nobody asked about: {baseline:?} -> {after:?}"
    );
}

/// A refused call leaves the project exactly as it found it.
///
/// Both refusals here are ones this campaign introduced, and both would be
/// worthless if they refused *after* writing: an argument KiCAD has no
/// constraint for, and a project file that does not exist.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn a_refusal_leaves_the_project_byte_for_byte() {
    let h = Harness::new();
    let board = h.write("untouched.kicad_pcb", harness::CLEARANCE_BOARD);
    let project = h.write("untouched.kicad_pro", harness::BLANK_PROJECT);
    let board_arg = harness::as_str(&board).to_string();
    let before_board = std::fs::read_to_string(&board).expect("readable");
    let before_project = std::fs::read_to_string(&project).expect("readable");

    for args in [
        json!({ "board": board_arg, "min_via_drill": 0.3 }),
        json!({ "board": board_arg, "min_clearance": -1.0 }),
    ] {
        let result = h
            .call("set_design_rules", args.clone())
            .await
            .expect("the tool answered");
        assert!(result.is_error, "{args} was accepted");
    }

    assert_eq!(
        std::fs::read_to_string(&board).expect("readable"),
        before_board,
        "a refused call edited the board"
    );
    assert_eq!(
        std::fs::read_to_string(&project).expect("readable"),
        before_project,
        "a refused call edited the project file"
    );
    // And KiCAD agrees the project is still usable, rather than merely
    // unchanged on disk.
    harness::kicad_reloads(&board);
}

/// A flip lands on the other side, and KiCAD is the one saying so.
///
/// KiCAD exposes no flip command over IPC, so there is nothing to compare
/// against and the tool has to edit the board directly — which puts it in
/// exactly the class this campaign found three defects in. The oracle is a
/// position file: `Side` is `top`/`bottom` in every locale, and it comes from
/// KiCAD's own reading of the board rather than from re-parsing our bytes.
///
/// The fixture's footprint is asymmetric on purpose. A symmetric one is
/// flipped correctly by an implementation that mirrors nothing at all.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn a_flip_lands_on_the_other_side_and_comes_back() {
    let h = Harness::new();
    let board = h.fixture("flip_pair.kicad_pcb");
    let board_arg = harness::as_str(&board).to_string();

    let before = harness::kicad_places(&board);
    let start = before.get("C310").expect("KiCAD places C310").clone();
    assert_eq!(start.side, "top", "the fixture does not start on the front");
    let original = std::fs::read_to_string(&board).expect("readable");

    h.json(
        "flip_component",
        json!({ "board": board_arg, "reference": "C310", "layer": "B.Cu" }),
    )
    .await;

    harness::kicad_reloads(&board);
    let flipped = harness::kicad_places(&board);
    let back = flipped.get("C310").expect("C310 survived the flip").clone();
    assert_eq!(back.side, "bottom", "the flip did not change sides");
    assert_eq!(
        (back.x, back.y),
        (start.x, start.y),
        "the flip moved the footprint; only its side was asked for"
    );
    assert_eq!(
        flipped.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>(),
        "the flip changed which footprints exist, or their references"
    );

    h.json(
        "flip_component",
        json!({ "board": board_arg, "reference": "C310", "layer": "F.Cu" }),
    )
    .await;

    harness::kicad_reloads(&board);
    let round_trip = harness::kicad_places(&board);
    assert_eq!(
        round_trip.get("C310"),
        Some(&start),
        "flipping back did not restore what KiCAD reported at the start"
    );
    assert_eq!(
        serialisation_noise(&std::fs::read_to_string(&board).expect("readable")),
        serialisation_noise(&original),
        "the round trip left the board different from how it found it"
    );
}

/// Flatten the two ways a flip round trip re-serialises without changing
/// anything KiCAD reads.
///
/// Measured on the round trip itself, not assumed: `(at 0 -1.8 0)` comes back
/// as `(at 0 -1.8)`, because the writer drops an explicit zero angle that
/// KiCAD treats as the default; and a rewritten `(effects …)` comes back with
/// one space before its closing paren. Every coordinate, uuid, layer and net
/// name came back identical — the geometry round-trips exactly, and only the
/// spelling moves.
///
/// Dropping ` 0)` everywhere is broader than those two cases — it would hide a
/// genuinely lost zero angle — which is why this is the *secondary* assertion.
/// The one that decides the test is KiCAD's own placement report above, and it
/// compares the rotation exactly.
fn serialisation_noise(board: &str) -> String {
    let mut flat = board.split_whitespace().collect::<Vec<_>>().join(" ");
    while flat.contains(" )") {
        flat = flat.replace(" )", ")");
    }
    flat.replace(" 0)", ")")
}
