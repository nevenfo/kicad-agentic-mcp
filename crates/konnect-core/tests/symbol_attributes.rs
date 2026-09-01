//! The three native symbol attributes — `in_bom`, `on_board`, `dnp` (W.3).
//!
//! KiCAD stores them as tags of the symbol block, not as properties, and
//! reads them from there for the BOM, the board update and the DNP marking.
//! Written as a property instead — `(property "dnp" "yes")` — they appear in
//! the symbol's field list and change nothing at all: a mutation that reports
//! success and has no effect, which is why this file asserts on the file's own
//! tags and not only on what the tool reports.
//!
//! An omitted tag is not an undecided one: KiCAD's defaults are `in_bom` and
//! `on_board` yes, `dnp` no. So the read path answers for all three whatever
//! the file carries, and the write path inserts a tag the symbol did not have.
//!
//! No `kicad-cli` and no running KiCAD: the file engine only.

mod harness;

use harness::{Harness, MULTIUNIT_LM2904, TWO_RESISTORS, TWO_RESISTORS_ONE_DNP};
use serde_json::json;

/// Whether the symbol block addressed by `uuid` carries `(name yes)`.
///
/// Read straight out of the file, never through the tool under test: the
/// lib_symbols definition at the top of the document carries `(in_bom yes)`
/// and `(on_board yes)` of its own, so a naive `content.contains("(dnp yes)")`
/// would be answering about the library entry half the time.
fn tag_of(content: &str, uuid: &str, name: &str) -> Option<bool> {
    let anchor = content
        .find(&format!("(uuid \"{uuid}\")"))
        .expect("the fixture carries that uuid");
    let block_start = content[..anchor]
        .rfind("\n  (symbol")
        .expect("the uuid sits inside a top-level symbol block");
    let block_end = block_start
        + content[block_start..]
            .find("\n  )")
            .expect("the symbol block is closed");
    let block = &content[block_start..block_end];
    let at = block.find(&format!("({name} "))?;
    match block[at..].split_whitespace().nth(1) {
        Some("yes)") => Some(true),
        Some("no)") => Some(false),
        other => panic!("unreadable ({name} …) tag: {other:?}"),
    }
}

const R1: &str = "f0000000-0000-4000-8000-000000000111";
const R2_DNP: &str = "f0000000-0000-4000-8000-000000000212";

#[tokio::test]
async fn reading_a_component_reports_all_three_attributes() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS_ONE_DNP);

    let plain = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch.to_str().unwrap(), "reference": "R1" }),
        )
        .await;
    // R1 carries `in_bom` and `on_board` and no `dnp` tag at all: the answer
    // for `dnp` is KiCAD's default, not an absent field.
    assert_eq!(plain["in_bom"], json!(true));
    assert_eq!(plain["on_board"], json!(true));
    assert_eq!(plain["dnp"], json!(false));

    let marked = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch.to_str().unwrap(), "reference": "R2" }),
        )
        .await;
    assert_eq!(marked["dnp"], json!(true));
}

#[tokio::test]
async fn listing_components_reports_all_three_attributes() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS_ONE_DNP);

    let listed = h
        .json(
            "list_schematic_components",
            json!({ "schematic": sch.to_str().unwrap() }),
        )
        .await;
    let by_ref = |reference: &str| -> serde_json::Value {
        listed["components"]
            .as_array()
            .expect("components is an array")
            .iter()
            .find(|c| c["reference"] == json!(reference))
            .expect("the fixture places that designator")
            .clone()
    };
    assert_eq!(by_ref("R1")["dnp"], json!(false));
    assert_eq!(by_ref("R2")["dnp"], json!(true));
    assert_eq!(by_ref("R2")["in_bom"], json!(true));
    assert_eq!(by_ref("R2")["on_board"], json!(true));
}

#[tokio::test]
async fn setting_an_attribute_the_symbol_does_not_carry_inserts_the_tag() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);

    let result = h
        .json(
            "edit_schematic_component",
            json!({ "schematic": sch.to_str().unwrap(), "reference": "R1", "dnp": true }),
        )
        .await;
    assert!(
        result["changes"]
            .as_array()
            .expect("changes is an array")
            .iter()
            .any(|c| c.as_str() == Some("dnp \u{2192} yes (added)")),
        "the insertion is reported as one: {result}"
    );

    let content = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(tag_of(&content, R1, "dnp"), Some(true));
    // Written as a tag of the symbol, never as a property KiCAD would ignore.
    assert!(
        !content.contains("(property \"dnp\""),
        "dnp was degraded into a custom property"
    );
    // In the place eeschema writes it — after `on_board`, before `uuid` — and
    // at the indentation the file already uses, so the diff of a one-attribute
    // edit is one line and the file still reads like a KiCAD file.
    //
    // Line endings are normalized first because they are not what this asserts.
    // `.gitattributes` says `text=auto`, so the fixture file arrives in CRLF on
    // a fresh Windows checkout and in LF here, and the engine writes back
    // whichever it was given — as it should. That preservation has its own
    // test, `a_crlf_sheet_is_written_back_as_crlf`.
    let normalized = content.replace("\r\n", "\n");
    assert!(
        normalized.contains("    (on_board yes)\n    (dnp yes)\n    (uuid \"f0000000-0000-4000-8000-000000000111\")"),
        "the tag is not where KiCAD would have written it: {content}"
    );
    // And the symbol still is what it was otherwise.
    assert!(content.contains("(property \"Value\" \"10k\""));
    assert!(content.contains("(instances (project \"\" (path \"/\" (reference \"R1\")"));
}

#[tokio::test]
async fn clearing_an_attribute_the_symbol_carries_rewrites_the_tag() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS_ONE_DNP);

    h.json(
        "edit_schematic_component",
        json!({ "schematic": sch.to_str().unwrap(), "reference": "R2", "dnp": false }),
    )
    .await;

    let content = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(tag_of(&content, R2_DNP, "dnp"), Some(false));
    // Cleared, not deleted: the tag stays, saying `no` out loud, which is what
    // eeschema writes for a symbol whose DNP was turned back off.
    let reread = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch.to_str().unwrap(), "reference": "R2" }),
        )
        .await;
    assert_eq!(reread["dnp"], json!(false));
}

#[tokio::test]
async fn the_three_attributes_can_be_set_in_one_call() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);

    h.json(
        "edit_schematic_component",
        json!({
            "schematic": sch.to_str().unwrap(),
            "reference": "R1",
            "in_bom": false,
            "on_board": false,
            "dnp": true,
            "value": "DNF"
        }),
    )
    .await;

    let content = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(tag_of(&content, R1, "in_bom"), Some(false));
    assert_eq!(tag_of(&content, R1, "on_board"), Some(false));
    assert_eq!(tag_of(&content, R1, "dnp"), Some(true));
    // A field edit in the same call still lands, and R2 is untouched.
    assert!(content.contains("(property \"Value\" \"DNF\""));
    let listed = h
        .json(
            "list_schematic_components",
            json!({ "schematic": sch.to_str().unwrap() }),
        )
        .await;
    let r2 = listed["components"]
        .as_array()
        .expect("components is an array")
        .iter()
        .find(|c| c["reference"] == json!("R2"))
        .expect("R2 is still placed")
        .clone();
    assert_eq!(r2["in_bom"], json!(true));
    assert_eq!(r2["dnp"], json!(false));
}

#[tokio::test]
async fn a_designator_addressed_attribute_reaches_every_unit() {
    let h = Harness::new();
    let sch = h.fixture(MULTIUNIT_LM2904);

    h.json(
        "edit_schematic_component",
        json!({ "schematic": sch.to_str().unwrap(), "reference": "U1", "dnp": true }),
    )
    .await;

    // Both units, not just the block a plain search lands on first: a file
    // where unit A is DNP and unit B is not contradicts itself, and KiCAD's
    // own editor sets the attribute on the whole component.
    let content = std::fs::read_to_string(&sch).expect("the schematic is readable");
    let dnp_tags = content.matches("(dnp yes)").count();
    assert_eq!(dnp_tags, 2, "both units carry the tag: {content}");
}

#[tokio::test]
async fn a_non_boolean_attribute_is_refused_rather_than_stored_as_text() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    // "yes" is what the file says, so it is the plausible wrong argument. The
    // `fields` map would have taken it and written a custom property.
    let result = h
        .call(
            "edit_schematic_component",
            json!({ "schematic": sch.to_str().unwrap(), "reference": "R1", "dnp": "yes" }),
        )
        .await
        .expect("the handler answers");
    assert!(result.is_error, "a string dnp is refused");

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(before, after, "the refused call wrote nothing");
}

#[tokio::test]
async fn the_batch_path_sets_attributes_on_several_components() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch.to_str().unwrap(),
                "edits": [
                    { "reference": "R1", "dnp": true, "in_bom": false },
                    { "reference": "R2", "on_board": false, "value": "22k" }
                ]
            }),
        )
        .await;
    assert_eq!(result["updated_count"], json!(2));
    assert_eq!(result["errors"], json!([]));

    let listed = h
        .json(
            "list_schematic_components",
            json!({ "schematic": sch.to_str().unwrap() }),
        )
        .await;
    let by_ref = |reference: &str| -> serde_json::Value {
        listed["components"]
            .as_array()
            .expect("components is an array")
            .iter()
            .find(|c| c["reference"] == json!(reference))
            .expect("the fixture places that designator")
            .clone()
    };
    assert_eq!(by_ref("R1")["dnp"], json!(true));
    assert_eq!(by_ref("R1")["in_bom"], json!(false));
    assert_eq!(by_ref("R1")["on_board"], json!(true));
    assert_eq!(by_ref("R2")["on_board"], json!(false));
    assert_eq!(by_ref("R2")["dnp"], json!(false));
    assert_eq!(by_ref("R2")["value"], json!("22k"));

    let content = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        !content.contains("(property \"on_board\""),
        "an attribute was degraded into a custom property"
    );
}

// ── Removing a property (W.3.5) ─────────────────────────────────────────────
//
// The counterpart of `set_symbol_property`'s insert-when-missing half. A
// property a client added by mistake — `exclude_from_board` on the Hi-Fi
// bench's `RV1` is the real one — was settable to the empty string and never
// removable, and an empty property is not no property: KiCAD still lists it in
// the symbol's fields and a BOM export still carries the column.

/// Give R1 a property no symbol should end up with, the way the tool that
/// created the real one did, and return the document as it was before —
/// which is what a removal has to restore, byte for byte.
async fn with_stray_property(h: &Harness, sch: &std::path::Path) -> String {
    let pristine = std::fs::read_to_string(sch).expect("the schematic is readable");
    h.json(
        "add_component_annotation",
        json!({
            "schematic": sch.to_str().unwrap(),
            "reference": "R1",
            "key": "exclude_from_board",
            "value": ""
        }),
    )
    .await;
    let content = std::fs::read_to_string(sch).expect("the schematic is readable");
    assert!(content.contains("(property \"exclude_from_board\""));
    pristine
}

#[tokio::test]
async fn a_null_field_removes_the_property() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);
    let pristine = with_stray_property(&h, &sch).await;
    let result = h
        .json(
            "edit_schematic_component",
            json!({
                "schematic": sch.to_str().unwrap(),
                "reference": "R1",
                "fields": { "exclude_from_board": null }
            }),
        )
        .await;
    assert!(
        result["changes"]
            .as_array()
            .expect("changes is an array")
            .iter()
            .any(|c| c.as_str() == Some("exclude_from_board removed")),
        "the removal is reported as one: {result}"
    );

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        !after.contains("exclude_from_board"),
        "the property is gone, not emptied: {after}"
    );
    // The whole `(property …)` block goes, its own lines with it — a property
    // eeschema wrote spans four lines, not one — and nothing else moves: the
    // document is byte for byte the one that existed before the property did.
    assert_eq!(after, pristine, "the removal was not a clean undo");
}

#[tokio::test]
async fn removing_a_property_the_symbol_never_had_is_refused() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    // Reported, not passed off as a change: a caller that misspelled the key
    // would otherwise be told its cleanup succeeded.
    let result = h
        .call(
            "edit_schematic_component",
            json!({
                "schematic": sch.to_str().unwrap(),
                "reference": "R1",
                "fields": { "Nonexistent_xyzzy": null }
            }),
        )
        .await
        .expect("the handler answers");
    assert!(result.is_error, "removing what is absent is refused");

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(before, after, "the refused call wrote nothing");
}

#[tokio::test]
async fn a_property_kicad_requires_cannot_be_removed() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    for field in ["Reference", "Value"] {
        let result = h
            .call(
                "edit_schematic_component",
                json!({
                    "schematic": sch.to_str().unwrap(),
                    "reference": "R1",
                    "fields": { field: null }
                }),
            )
            .await
            .expect("the handler answers");
        assert!(result.is_error, "'{field}' cannot be removed");
    }

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(before, after, "the refused calls wrote nothing");
}

#[tokio::test]
async fn a_removal_reaches_every_unit_of_a_multiunit_symbol() {
    let h = Harness::new();
    let sch = h.fixture(MULTIUNIT_LM2904);
    h.json(
        "add_component_annotation",
        json!({
            "schematic": sch.to_str().unwrap(),
            "reference": "U1",
            "key": "Stray",
            "value": "x"
        }),
    )
    .await;
    let seeded = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(seeded.matches("(property \"Stray\"").count(), 2);

    h.json(
        "edit_schematic_component",
        json!({
            "schematic": sch.to_str().unwrap(),
            "reference": "U1",
            "fields": { "Stray": null }
        }),
    )
    .await;

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(!after.contains("Stray"), "both units lost the property");
}

#[tokio::test]
async fn the_batch_path_removes_a_property_too() {
    let h = Harness::new();
    let sch = h.fixture(TWO_RESISTORS);
    let _ = with_stray_property(&h, &sch).await;

    let result = h
        .json(
            "batch_edit_schematic_components",
            json!({
                "schematic": sch.to_str().unwrap(),
                "edits": [
                    { "reference": "R1", "fields": { "exclude_from_board": null }, "on_board": false }
                ]
            }),
        )
        .await;
    assert_eq!(result["errors"], json!([]));

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(!after.contains("exclude_from_board"));
    assert_eq!(tag_of(&after, R1, "on_board"), Some(false));
}
