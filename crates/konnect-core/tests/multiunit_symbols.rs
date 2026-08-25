//! A multi-unit symbol (P.6.8.2) — the "edit" half of #179.
//!
//! `MULTIUNIT_LM2904` places one real `Amplifier_Operational:LM2904` as `U1`
//! with two units, each its own top-level `(symbol …)` block sharing the
//! `U1` designator. Measured on this fixture (10.0.3): desynchronising
//! `Value` and exporting the netlist reports the *first* unit's value
//! whichever unit was actually edited — KiCad's own netlist export reads
//! only the first block. A tool that reads or writes only the first block
//! is therefore either accidentally correct or silently wrong, and always
//! leaves a self-contradicting file. These tests assert the four different
//! answers a reference-addressed call now gives depending on what kind of
//! operation it is (P.6.8.2's design): a property write reaches every unit,
//! a delete drops every unit, a geometry call refuses outright, and a read
//! says which unit it resolved.
//!
//! No `kicad-cli` and no running KiCAD: the file engine only.

mod harness;

use harness::{Harness, MULTIUNIT_LM2904, TWO_RESISTORS};
use serde_json::json;

/// Both units' own `(property "Reference" "U1" …)` blocks, so a test can
/// read each unit's current `Value` independently of what any tool reports.
fn unit_values(content: &str) -> Vec<String> {
    content
        .match_indices(r#"(property "Value" ""#)
        .map(|(pos, needle)| {
            let start = pos + needle.len();
            let end = start + content[start..].find('"').expect("closing quote");
            content[start..end].to_string()
        })
        .collect()
}

// ─── Property write reaches every unit ───────────────────────────────────────

/// `edit_schematic_component`'s `value` change lands on both of `U1`'s units,
/// not just the first — red before P.6.8.2 (a single-block write left unit 2
/// stale), green after.
#[tokio::test]
async fn editing_value_by_reference_updates_every_unit() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();

    h.json(
        "edit_schematic_component",
        json!({ "schematic": &sch, "reference": "U1", "value": "LM2904X" }),
    )
    .await;

    let content = std::fs::read_to_string(&sch).unwrap();
    let values = unit_values(&content);
    // Three `Value` properties total: the fixture's own `lib_symbols`
    // definition (untouched — it carries no `lib_id`, so
    // `find_all_symbol_instance_blocks`'s discriminator skips it) plus the
    // two placed units.
    assert_eq!(values.len(), 3, "{values:?}");
    let placed: Vec<&String> = values.iter().filter(|v| v.as_str() != "LM2904").collect();
    assert_eq!(
        placed,
        vec!["LM2904X", "LM2904X"],
        "both placed units carry the new value, not just the first: {values:?}"
    );
}

// ─── Delete drops every unit ─────────────────────────────────────────────────

/// `delete_schematic_component` on `U1` leaves no `U1` block behind — neither
/// unit survives as a half-deleted component.
#[tokio::test]
async fn deleting_by_reference_removes_every_unit() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();

    let result = h
        .json(
            "delete_schematic_component",
            json!({ "schematic": &sch, "reference": "U1" }),
        )
        .await;
    assert_eq!(result["units_deleted"], 2, "{result}");

    let content = std::fs::read_to_string(&sch).unwrap();
    assert!(
        !content.contains(r#"(property "Reference" "U1""#),
        "no U1 block should remain: {content}"
    );
}

// ─── Geometry refuses on a multi-unit symbol, but not on a single-unit one ───

/// `move_schematic_component` addressed by the shared `reference` refuses on
/// a multi-unit symbol, naming both units' uuids rather than silently moving
/// the first.
#[tokio::test]
async fn moving_a_multiunit_symbol_by_reference_is_refused_and_names_the_units() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();
    let before = std::fs::read_to_string(&sch).unwrap();

    let result = h
        .call(
            "move_schematic_component",
            json!({ "schematic": &sch, "reference": "U1", "x": 200.0, "y": 200.0 }),
        )
        .await
        .unwrap();
    assert!(
        result.is_error,
        "a multi-unit reference move must be refused"
    );
    let text = harness::body(&result).to_string();
    assert!(
        text.contains("f0000000-0000-4000-8000-000000000021")
            && text.contains("f0000000-0000-4000-8000-000000000022"),
        "the refusal names both units' uuids: {text}"
    );

    let after = std::fs::read_to_string(&sch).unwrap();
    assert_eq!(before, after, "a refused move must not touch the file");
}

/// The same call on a single-unit fixture is unaffected: no ambiguity, no
/// refusal.
#[tokio::test]
async fn moving_a_single_unit_symbol_by_reference_still_moves() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    let result = h
        .json(
            "move_schematic_component",
            json!({ "schematic": &sch, "reference": "R1", "x": 128.27, "y": 60.96 }),
        )
        .await;
    assert_eq!(result["moved"], "R1", "{result}");
    assert_eq!(result["x"], 128.27, "{result}");
}

/// Addressing one unit by its own `uuid` moves that unit alone — the other
/// unit's position is untouched.
#[tokio::test]
async fn moving_one_unit_by_uuid_moves_only_that_unit() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();
    const UNIT2_UUID: &str = "f0000000-0000-4000-8000-000000000022";

    h.json(
        "move_schematic_component",
        json!({ "schematic": &sch, "uuid": UNIT2_UUID, "x": 127.0, "y": 50.8 }),
    )
    .await;

    let content = std::fs::read_to_string(&sch).unwrap();
    assert!(
        content.contains("(at 127 50.8 0)"),
        "unit 2 moved to its new position: {content}"
    );
    assert!(
        content.contains("(at 100 50 0)"),
        "unit 1 stayed where it was: {content}"
    );
}

// ─── Read says which unit it resolved ────────────────────────────────────────

/// `get_schematic_component` addressed by the shared `reference` resolves to
/// the first unit, and now says so: `unit` and the sibling's uuid are both in
/// the answer.
#[tokio::test]
async fn getting_a_multiunit_component_by_reference_names_the_resolved_unit() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();

    let result = h
        .json(
            "get_schematic_component",
            json!({ "schematic": &sch, "reference": "U1" }),
        )
        .await;

    assert_eq!(result["unit"], 1, "{result}");
    assert_eq!(
        result["uuid"], "f0000000-0000-4000-8000-000000000021",
        "{result}"
    );
    let siblings = result["sibling_unit_uuids"]
        .as_array()
        .expect("sibling_unit_uuids is an array");
    assert_eq!(
        siblings,
        &vec![json!("f0000000-0000-4000-8000-000000000022")],
        "{result}"
    );
}
