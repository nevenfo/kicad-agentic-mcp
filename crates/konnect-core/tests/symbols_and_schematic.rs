//! The symbol and sheet surface, exercised end to end (J.2.3.2).
//!
//! Seventeen `symbols` and `schematic` tools shipped with no test that runs.
//! These drive them through the router by name against a fixture with two real
//! `Device:R` symbols, and assert on what changed in the document rather than
//! on the tool returning without error.
//!
//! `annotate_schematic` and `get_schematic_view` are absent on purpose: both
//! shell out to `kicad-cli` and belong with the other `cli` tools in J.2.3.7.
//!
//! No `kicad-cli` and no running KiCAD: the file engine only.

mod harness;

use harness::{pins, Harness, TWO_RESISTORS};
use serde_json::json;

async fn sheet(h: &Harness) -> String {
    harness::as_str(&h.fixture(TWO_RESISTORS)).to_string()
}

// ─── Reading one component ───────────────────────────────────────────────────

/// `get_schematic_component` finds a component by reference and reports where
/// it is, and says so rather than inventing one when the reference is unknown.
#[tokio::test]
async fn a_component_is_found_by_reference_and_a_missing_one_is_not_invented() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let r1 = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    let text = r1.to_string();
    assert!(text.contains("101.6"), "R1 is at x = 101.6: {r1}");
    assert!(text.contains("10k"), "R1's value is 10k: {r1}");

    let missing = h
        .call(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R99" }),
        )
        .await;
    let reported = match missing {
        Ok(result) => harness::body(&result).to_string(),
        Err(e) => e.to_string(),
    };
    assert!(
        !reported.contains("101.6"),
        "R99 does not exist and must not come back as R1: {reported}"
    );
}

/// `batch_get_schematic_pin_locations` is the batched form of the pin lookup
/// the golden benchmark relies on, and its answers must be the same pins.
#[tokio::test]
async fn batched_pin_locations_match_the_fixtures_pins() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let located = h
        .json(
            "batch_get_schematic_pin_locations",
            json!({ "schematic": sch, "references": ["R1", "R2"] }),
        )
        .await;
    let components = located["components"]
        .as_array()
        .expect("the batch answers per component");
    assert_eq!(
        components.len(),
        2,
        "both references were asked for: {located}"
    );
    for (component, expected) in components.iter().zip([pins::R1_PIN1, pins::R2_PIN1]) {
        let pin1 = &component["pins"][0];
        let (x, y) = (
            pin1["x"].as_f64().expect("a pin has an x"),
            pin1["y"].as_f64().expect("a pin has a y"),
        );
        assert!(
            (x - expected.0).abs() < 0.01 && (y - expected.1).abs() < 0.01,
            "pin 1 of {} is at ({x}, {y}), not ({}, {})",
            component["reference"],
            expected.0,
            expected.1
        );
    }
}

/// `get_schematic_layout` is the whole-sheet view, and its optional sections
/// are actually optional — a caller asking for less must get less, since the
/// point of the flags is context cost.
#[tokio::test]
async fn the_layout_view_honours_its_include_flags() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    h.json(
        "add_wire",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN1.0, "y1": pins::R1_PIN1.1,
            "x2": pins::R2_PIN1.0, "y2": pins::R2_PIN1.1
        }),
    )
    .await;

    let full = h
        .json("get_schematic_layout", json!({ "schematic": sch }))
        .await;
    assert!(
        full["wires"].as_array().is_some_and(|w| !w.is_empty()),
        "the default view includes the wire: {full}"
    );

    let trimmed = h
        .json(
            "get_schematic_layout",
            json!({ "schematic": sch, "include_wires": false, "include_labels": false }),
        )
        .await;
    assert!(
        trimmed["wires"].as_array().is_none_or(|w| w.is_empty()),
        "include_wires: false still returned wires: {trimmed}"
    );
    assert!(
        trimmed.to_string().contains("R1"),
        "the components are the part that is never optional: {trimmed}"
    );
}

// ─── Editing one component ───────────────────────────────────────────────────

/// An edit reaches the file, and a rename is a rename: the old reference stops
/// resolving.
#[tokio::test]
async fn editing_a_component_renames_and_revalues_it() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let edited = h
        .json(
            "edit_schematic_component",
            json!({
                "schematic": sch,
                "reference": "R2",
                "new_reference": "R7",
                "value": "47k"
            }),
        )
        .await;
    assert!(
        edited["errors"].is_null(),
        "both fields are on one symbol and neither should fail — the rename used \
         to go first and made the symbol unfindable for the rest of the call: {edited}"
    );

    let renamed = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R7" }),
        )
        .await;
    assert_eq!(
        renamed["value"], "47k",
        "the value did not follow the rename: {renamed}"
    );

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        !text.contains("\"R2\""),
        "the old reference survived the rename:\n{text}"
    );
}

/// A symbol carries only the properties it was given, and the fixture's
/// resistors have no `Footprint` at all. Setting one has to add it — refusing
/// made the tool unable to do the commonest edit after placement (J.2.4.1).
#[tokio::test]
async fn a_field_the_symbol_does_not_have_yet_is_added_rather_than_refused() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let edited = h
        .json(
            "edit_schematic_component",
            json!({
                "schematic": sch,
                "reference": "R1",
                "footprint": "Resistor_SMD:R_0603_1608Metric"
            }),
        )
        .await;
    assert!(
        edited["errors"].is_null(),
        "a missing property is added, not reported as an error: {edited}"
    );

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        text.contains("R_0603_1608Metric"),
        "the footprint was not written:\n{text}"
    );

    // And setting it again edits the property rather than adding a second one.
    h.json(
        "edit_schematic_component",
        json!({
            "schematic": sch,
            "reference": "R1",
            "footprint": "Resistor_SMD:R_0805_2012Metric"
        }),
    )
    .await;
    let again = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(
        again.matches("(property \"Footprint\"").count(),
        1,
        "R1 ended up with two Footprint properties:\n{again}"
    );
    assert!(
        again.contains("R_0805_2012Metric") && !again.contains("R_0603_1608Metric"),
        "the second edit did not replace the first:\n{again}"
    );
}

/// `add_component_annotation` adds a free-form property, which is how a caller
/// carries information KiCAD has no field for.
#[tokio::test]
async fn an_annotation_is_stored_as_a_property_on_the_component() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "add_component_annotation",
        json!({
            "schematic": sch,
            "reference": "R1",
            "key": "MPN",
            "value": "RC0603FR-0710KL"
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(text.contains("\"MPN\""), "the property name is missing");
    assert!(
        text.contains("RC0603FR-0710KL"),
        "the property value is missing"
    );
}

/// A move puts the component where it was told, and rotation is absolute — the
/// argument is a heading, not an increment.
#[tokio::test]
async fn moving_and_rotating_are_absolute() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "move_schematic_component",
        json!({ "schematic": sch, "reference": "R1", "x": 152.4, "y": 76.2 }),
    )
    .await;
    let moved = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    assert!(
        moved.to_string().contains("152.4"),
        "R1 did not move: {moved}"
    );

    for angle in [90, 90] {
        h.json(
            "rotate_schematic_component",
            json!({ "schematic": sch, "reference": "R1", "rotation": angle }),
        )
        .await;
    }
    let rotated = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    assert!(
        !rotated.to_string().contains("180"),
        "rotating to 90 twice landed at 180 — the argument is absolute: {rotated}"
    );
}

/// Deleting a component takes it out of the document; the other one stays.
#[tokio::test]
async fn deleting_a_component_leaves_the_others_alone() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "delete_schematic_component",
        json!({ "schematic": sch, "reference": "R1" }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(!text.contains("\"R1\""), "R1 survived its deletion");
    assert!(text.contains("\"R2\""), "R2 was taken with it");
}

// ─── Moving several at once ──────────────────────────────────────────────────

/// `move_connected` moves the symbol *and* drags the wire ends that were on
/// its pins (J.2.4.2). Before that it delegated to the plain move and left
/// every attached wire behind, silently breaking the connection the caller was
/// trying to preserve.
#[tokio::test]
async fn moving_connected_drags_the_attached_wire_end_along() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    h.json(
        "add_wire",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN1.0, "y1": pins::R1_PIN1.1,
            "x2": pins::R2_PIN1.0, "y2": pins::R2_PIN1.1
        }),
    )
    .await;

    let result = h
        .json(
            "move_connected",
            json!({ "schematic": sch, "reference": "R1", "x": 88.9, "y": 50.8 }),
        )
        .await;
    assert_eq!(
        result["wire_ends_dragged"], 1,
        "one wire end was on R1's pin 1: {result}"
    );

    let moved = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    assert_eq!(moved["x"], 88.9, "the symbol moved: {moved}");

    let wires = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    let wire = &wires["wires"][0];
    let (x1, x2) = (
        wire["x1"].as_f64().expect("the wire reports its start"),
        wire["x2"].as_f64().expect("the wire reports its end"),
    );
    assert!(
        (x1 - 88.9).abs() < 0.01,
        "the end on R1's pin should have followed it to 88.9: {wires}"
    );
    assert!(
        (x2 - pins::R2_PIN1.0).abs() < 0.01,
        "the end on R2 must not have moved: {wires}"
    );
}

/// Only the ends that were on a moved pin are dragged: a wire elsewhere on the
/// sheet stays where it was drawn.
#[tokio::test]
async fn a_wire_that_touches_nothing_is_left_alone_by_move_connected() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    h.json(
        "add_wire",
        json!({ "schematic": sch, "x1": 25.4, "y1": 25.4, "x2": 38.1, "y2": 25.4 }),
    )
    .await;

    let result = h
        .json(
            "move_connected",
            json!({ "schematic": sch, "reference": "R1", "x": 88.9, "y": 50.8 }),
        )
        .await;
    assert_eq!(
        result["wire_ends_dragged"], 0,
        "nothing was attached to R1: {result}"
    );

    let wires = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    assert_eq!(
        wires["wires"][0]["x1"], 25.4,
        "the loose wire moved: {wires}"
    );
}

/// `bulk_move_schematic_components` and `move_region` both shift by a delta.
/// A region move must take what is inside the box and leave what is outside.
#[tokio::test]
async fn a_bulk_move_shifts_by_a_delta_and_a_region_move_respects_its_box() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "bulk_move_schematic_components",
        json!({ "schematic": sch, "references": ["R1", "R2"], "dx": 12.7, "dy": 0 }),
    )
    .await;
    let r1 = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    assert!(
        r1.to_string().contains("114.3"),
        "R1 should have moved 101.6 -> 114.3: {r1}"
    );

    // R1 is now at 114.3 and R2 at 127. A box around R1 alone must move only it.
    h.json(
        "move_region",
        json!({
            "schematic": sch,
            "x1": 110.0, "y1": 40.0, "x2": 120.0, "y2": 60.0,
            "dx": 0, "dy": 12.7
        }),
    )
    .await;
    let moved = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R1" }),
        )
        .await;
    let untouched = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "reference": "R2" }),
        )
        .await;
    assert!(
        moved.to_string().contains("63.5"),
        "R1 was inside the box and should have moved down: {moved}"
    );
    assert!(
        untouched.to_string().contains("50.8"),
        "R2 was outside the box and must not have moved: {untouched}"
    );
}

// ─── Batches ─────────────────────────────────────────────────────────────────

/// One call, several edits — and the batch has to apply all of them, not the
/// first.
#[tokio::test]
async fn a_batch_edit_applies_every_edit_in_it() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "batch_edit_schematic_components",
        json!({
            "schematic": sch,
            "edits": [
                { "reference": "R1", "value": "1k" },
                { "reference": "R2", "value": "2k2" }
            ]
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(text.contains("\"1k\""), "R1's edit was not applied");
    assert!(text.contains("\"2k2\""), "R2's edit was not applied");
}

/// A batch spec's `fields` map is the same generic property path
/// `edit_schematic_component` got in P.6.9.6, and it has to behave the same
/// way here: KiCAD stores every property as text, so a JSON number is written
/// as its text form instead of being dropped for not being a string.
#[tokio::test]
async fn a_batch_edit_writes_a_field_given_as_a_number() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch,
                "edits": [ { "reference": "R1", "fields": { "Qty": 2 } } ]
            }),
        )
        .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        text.contains(r#"(property "Qty" "2""#),
        "a numeric field value must be written as its text form: {result}"
    );
}

/// A value with no single-line text form cannot become a property, and saying
/// so beats writing nothing while the batch reports success.
#[tokio::test]
async fn a_batch_edit_refuses_a_field_value_with_no_text_form() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch,
                "edits": [ { "reference": "R1", "fields": {
                    "Spec": { "nested": 1 },
                    "Tags": ["a", "b"],
                    "Empty": null
                } } ]
            }),
        )
        .await;

    let errors = result["errors"].to_string();
    for key in ["Spec", "Tags", "Empty"] {
        assert!(
            errors.contains(key),
            "'{key}' has no text form and must be reported, not ignored: {result}"
        );
    }
    assert_eq!(
        before,
        std::fs::read_to_string(&sch).expect("the schematic is readable"),
        "a refused value must leave the sheet alone"
    );
}

/// J.2.4.1 removed "the symbol does not carry this field yet" as a reason to
/// refuse an edit on the single-component path; the batch path refused it too.
#[tokio::test]
async fn a_batch_edit_adds_a_field_the_symbol_does_not_carry_yet() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch,
                "edits": [ { "reference": "R1", "fields": { "MPN": "RC0805FR-0710KL" } } ]
            }),
        )
        .await;

    assert!(
        !result["errors"].to_string().contains("not found"),
        "a missing property is created, not refused: {result}"
    );
    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        text.contains(r#"(property "MPN" "RC0805FR-0710KL""#),
        "the new property was not inserted: {result}"
    );
}

/// A key the symbol already carries is rewritten where it stands: a second
/// `(property "Value" …)` on the same symbol is a schematic KiCAD reads
/// differently from the one that was asked for.
#[tokio::test]
async fn a_batch_edit_updates_an_existing_field_in_place_without_duplicating_it() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");
    let value_properties = before.matches(r#"(property "Value""#).count();

    h.json(
        "batch_edit_schematic_components",
        json!({
            "schematic": sch,
            "edits": [ { "reference": "R1", "fields": { "Value": "22k" } } ]
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(
        text.matches(r#""22k""#).count(),
        1,
        "the update must land once, in place"
    );
    assert_eq!(
        text.matches(r#"(property "Value""#).count(),
        value_properties,
        "updating a field must not add a second one"
    );
}

/// D124: `Reference` is the one property stored twice — as a property and
/// inside `(instances …)` — so the generic `fields` path, which rewrites only
/// the property, must not touch it at all.
#[tokio::test]
async fn a_batch_edit_refuses_to_rewrite_the_reference_property() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch,
                "edits": [ { "reference": "R1", "fields": { "Reference": "R9" } } ]
            }),
        )
        .await;

    assert!(
        result["errors"].to_string().contains("Reference"),
        "renaming through 'fields' must be refused out loud: {result}"
    );
    assert_eq!(
        before,
        std::fs::read_to_string(&sch).expect("the schematic is readable"),
        "a refused rename must write nothing"
    );
}

/// P.6.9.4's rule, on this path: one field on one component is one line of
/// diff, not a reserialised document.
#[tokio::test]
async fn a_batch_edit_of_one_field_changes_one_line_of_the_sheet() {
    let h = Harness::new();
    let sch = sheet(&h).await;
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    h.json(
        "batch_edit_schematic_components",
        json!({
            "schematic": sch,
            "edits": [ { "reference": "R1", "fields": { "Value": "22k" } } ]
        }),
    )
    .await;

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    assert_eq!(
        before_lines.len(),
        after_lines.len(),
        "an in-place value edit changes no line count"
    );
    let differing: Vec<usize> = before_lines
        .iter()
        .zip(&after_lines)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        differing.len(),
        1,
        "exactly the edited property line differs, got lines {differing:?}"
    );
}

/// The batch delete empties the sheet of exactly what it was given.
#[tokio::test]
async fn a_batch_delete_removes_every_reference_it_is_given() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "batch_delete_schematic_components",
        json!({ "schematic": sch, "references": ["R1", "R2"] }),
    )
    .await;

    let layout = h
        .json("get_schematic_layout", json!({ "schematic": sch }))
        .await;
    let remaining = layout["components"]
        .as_array()
        .map(|c| c.len())
        .unwrap_or(0);
    assert_eq!(
        remaining, 0,
        "components survived the batch delete: {layout}"
    );
}

/// `group_components` tags a set with a shared name, which is how a caller
/// keeps a sub-circuit identifiable across later calls.
#[tokio::test]
async fn grouping_tags_every_member_with_the_group_name() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "group_components",
        json!({
            "schematic": sch,
            "references": ["R1", "R2"],
            "group_name": "INPUT_DIVIDER"
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(
        text.matches("INPUT_DIVIDER").count(),
        2,
        "both components should carry the group name:\n{text}"
    );
}

/// Grouping the same component twice must not leave it carrying two `Group`
/// properties. `add_component_annotation` and `edit_schematic_component` were
/// taught to update-or-insert in P.6.9.5; this path was outside that item's
/// scope and kept appending unconditionally.
#[tokio::test]
async fn regrouping_a_component_updates_its_group_rather_than_adding_a_second() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    for name in ["INPUT_DIVIDER", "OUTPUT_DIVIDER"] {
        h.json(
            "group_components",
            json!({
                "schematic": sch,
                "references": ["R1"],
                "group_name": name
            }),
        )
        .await;
    }

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(
        text.matches(r#"(property "Group""#).count(),
        1,
        "a second grouping must replace the first, not stack on it:
{text}"
    );
    assert_eq!(
        text.matches("INPUT_DIVIDER").count(),
        0,
        "the superseded group name must be gone:
{text}"
    );
}

/// A grouped component's `Group` text belongs on the component, not at the
/// sheet origin: the property is written at a hardcoded `(at 0 0 0)` unless it
/// is anchored on the symbol's own position.
#[tokio::test]
async fn a_group_property_is_anchored_on_the_symbol_not_the_sheet_origin() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "group_components",
        json!({
            "schematic": sch,
            "references": ["R1"],
            "group_name": "INPUT_DIVIDER"
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    let group_at = text
        .split_once(r#"(property "Group""#)
        .and_then(|(_, rest)| rest.split_once("(at "))
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(at, _)| at.trim().to_string())
        .expect("the Group property carries an (at ...)");
    assert_ne!(
        group_at, "0 0 0",
        "the group text was written at the sheet origin:
{text}"
    );
}

/// `add_schematic_text` is an annotation on the sheet, not on a component.
#[tokio::test]
async fn sheet_text_is_written_where_it_was_placed() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "add_schematic_text",
        json!({
            "schematic": sch,
            "text": "Input divider — do not populate R2",
            "x": 50.8, "y": 25.4
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(text.contains("(text"), "no text node was written");
    assert!(
        text.contains("do not populate R2"),
        "the text content is missing"
    );
}

// ─── Sheet-level checks ──────────────────────────────────────────────────────

/// Two checkers that have to be able to report "nothing wrong": the fixture is
/// two separate resistors with no overlap, and orphan detection is advisory
/// (E7), so this pins the shape of the answer and not a verdict.
#[tokio::test]
async fn the_sheet_checkers_report_a_clean_fixture_as_clean() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    let overlaps = h
        .json("check_schematic_overlaps", json!({ "schematic": sch }))
        .await;
    let found = overlaps["overlaps"]
        .as_array()
        .map(|o| o.len())
        .unwrap_or(0);
    assert_eq!(
        found, 0,
        "two resistors 12.7 mm apart do not overlap: {overlaps}"
    );

    let orphans = h
        .json("find_orphan_items", json!({ "schematic": sch }))
        .await;
    assert!(
        orphans.is_object(),
        "find_orphan_items returned no report: {orphans}"
    );
}

/// Overlap detection has to find one when there is one — a checker that always
/// answers "clean" passes the test above and is worthless.
#[tokio::test]
async fn overlap_detection_finds_a_component_placed_on_another() {
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "move_schematic_component",
        json!({ "schematic": sch, "reference": "R2", "x": 101.6, "y": 50.8 }),
    )
    .await;

    let overlaps = h
        .json("check_schematic_overlaps", json!({ "schematic": sch }))
        .await;
    let found = overlaps["overlaps"]
        .as_array()
        .map(|o| o.len())
        .unwrap_or(0);
    assert!(
        found > 0,
        "R2 was moved on top of R1 and nothing was reported: {overlaps}"
    );
}

// ─── What KiCAD actually reads ───────────────────────────────────────────────

/// A rename has to be a rename *to KiCAD*, not just in the property an editor
/// displays.
///
/// KiCAD resolves a symbol's designator from its `instances` block. Renaming
/// only the Reference property left `kicad-cli sch export netlist` still
/// emitting the old designator while the tool reported `Reference → R7` — an
/// agent renaming a part would have believed it. `#[ignore]`d because it needs
/// a real `kicad-cli`:
///
///     KICAD_CLI=<path> cargo test -p konnect-core --test symbols_and_schematic -- --ignored
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn kicad_sees_a_renamed_component_under_its_new_designator() {
    let cli = kicad_cli();
    let h = Harness::new();
    let sch = sheet(&h).await;

    h.json(
        "edit_schematic_component",
        json!({ "schematic": sch, "reference": "R2", "new_reference": "R7", "value": "47k" }),
    )
    .await;

    let netlist = h.path("renamed.net");
    let output = std::process::Command::new(&cli)
        .args(["sch", "export", "netlist", "--output"])
        .arg(&netlist)
        .arg(&sch)
        .output()
        .expect("kicad-cli runs");
    assert!(
        output.status.success(),
        "netlist export failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(&netlist).expect("the netlist is readable");
    assert!(
        text.contains(r#"(ref "R7")"#),
        "KiCAD does not know the part as R7:\n{text}"
    );
    assert!(
        !text.contains(r#"(ref "R2")"#),
        "KiCAD still knows the part as R2 — the instances block was not repointed:\n{text}"
    );
}

fn kicad_cli() -> String {
    if let Ok(path) = std::env::var("KICAD_CLI") {
        assert!(
            std::path::Path::new(&path).exists(),
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
        .find(|path| std::path::Path::new(path).exists())
        .map(|path| path.to_string())
        .unwrap_or_else(|| panic!("no kicad-cli found — set KICAD_CLI to run this probe"))
}
