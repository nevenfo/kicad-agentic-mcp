//! D.4's validation, against a whole project rather than a fixture.
//!
//! The per-toolset tests prove that each tool accepts a uuid. What they cannot
//! prove is that a caller can *work* that way: obtain an address from a
//! reading tool, edit through it, and still have it resolve afterwards. That
//! loop is what this probe runs, on `bench/fixtures/divider.kicad_sch` — the
//! ERC-clean divider the benchmark builds its runs on, with six symbols, a
//! wire, a net label, power symbols and PWR_FLAGs. It is the most complete
//! document the repository carries; it was written by Konnect rather than by
//! eeschema, which is worth knowing when reading what this proves.
//!
//! The document is copied first: a probe never edits the repository's file.
//!
//! No `kicad-cli` and no running KiCAD.
mod harness;

use harness::{as_str, Harness};
use serde_json::json;

const DIVIDER: &str = "bench/fixtures/divider.kicad_sch";

/// The uuid a reading tool publishes for `reference`.
async fn component_uuid(h: &Harness, sch: &str, reference: &str) -> String {
    let listed = h
        .json("list_schematic_components", json!({ "schematic": sch }))
        .await;
    listed["components"]
        .as_array()
        .expect("the list is an array")
        .iter()
        .find(|c| c["reference"] == json!(reference))
        .unwrap_or_else(|| panic!("{reference} is in the divider"))["uuid"]
        .as_str()
        .expect("the list publishes a uuid")
        .to_string()
}

/// The whole point of D.4: an address that outlives the thing moving.
///
/// A reference designator survives a move too — what it does not survive is a
/// rename, which is why the second half renames R1 and addresses it again by
/// the uuid it had all along.
#[tokio::test]
async fn a_uuid_obtained_from_a_listing_survives_a_move_and_a_rename() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    let uuid = component_uuid(&h, sch, "R1").await;

    let moved = h
        .json(
            "move_schematic_component",
            json!({ "schematic": sch, "uuid": uuid, "x": 120.65, "y": 80.01 }),
        )
        .await;
    assert!(moved.get("error").is_none(), "{moved}");

    let after_move = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "uuid": uuid }),
        )
        .await;
    assert_eq!(after_move["x"], json!(120.65));
    assert_eq!(after_move["y"], json!(80.01));
    assert_eq!(after_move["reference"], json!("R1"));

    // A rename is where the two address forms part company.
    let renamed = h
        .json(
            "edit_schematic_component",
            json!({ "schematic": sch, "uuid": uuid, "new_reference": "R10" }),
        )
        .await;
    assert!(renamed.get("error").is_none(), "{renamed}");

    let after_rename = h
        .json(
            "get_schematic_component",
            json!({ "schematic": sch, "uuid": uuid }),
        )
        .await;
    assert_eq!(
        after_rename["reference"],
        json!("R10"),
        "the uuid still names the symbol its old designator no longer does"
    );
    assert_eq!(after_rename["x"], json!(120.65));
}

/// The same loop for a label, which has no designator to fall back on: before
/// D.4.1.4 the only way to name one was its net name plus its position, so an
/// edit that moved it made the next call guess.
#[tokio::test]
async fn a_label_uuid_from_the_listing_addresses_the_rotate_tool() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    let listed = h
        .json("list_schematic_labels", json!({ "schematic": sch }))
        .await;
    let label = listed["labels"]
        .as_array()
        .expect("the list is an array")
        .iter()
        .find(|l| l["net"] == json!("VOUT"))
        .expect("the divider labels its midpoint VOUT")
        .clone();
    let uuid = label["uuid"].as_str().expect("published").to_string();

    let rotated = h
        .json(
            "rotate_schematic_label",
            json!({ "schematic": sch, "uuid": uuid, "rotation": 90.0 }),
        )
        .await;
    assert!(rotated.get("error").is_none(), "{rotated}");

    let again = h
        .json("list_schematic_labels", json!({ "schematic": sch }))
        .await;
    let same = again["labels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["uuid"] == json!(uuid))
        .expect("the uuid did not change under the edit");
    assert_eq!(same["rotation"], json!(90.0));
    assert_eq!(same["net"], json!("VOUT"));
}

/// Editing through a uuid must not renumber the document: every other item
/// keeps the identity it had, or every address a caller holds goes stale at
/// once.
#[tokio::test]
async fn editing_through_a_uuid_leaves_every_other_identity_alone() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    let before: Vec<String> = h
        .json("list_schematic_components", json!({ "schematic": sch }))
        .await["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["uuid"].as_str().unwrap().to_string())
        .collect();
    assert!(before.len() >= 6, "the divider carries six symbols");

    let r2 = component_uuid(&h, sch, "R2").await;
    h.json(
        "move_schematic_component",
        json!({ "schematic": sch, "uuid": r2, "x": 130.81, "y": 95.25 }),
    )
    .await;

    let after: Vec<String> = h
        .json("list_schematic_components", json!({ "schematic": sch }))
        .await["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["uuid"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(before, after, "an edit rewrites no identity but its own");

    // And the document is still one KiCAD can read.
    let text = std::fs::read_to_string(sch).unwrap();
    assert!(konnect_sexp::parse_sexp(&text).is_ok());
}

/// An address that resolves to nothing says so, and says what is there — the
/// only answer a caller holding a stale uuid can act on.
#[tokio::test]
async fn a_stale_uuid_is_not_found_and_names_the_candidates() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    let result = h
        .call(
            "get_schematic_component",
            json!({ "schematic": sch, "uuid": "00000000-0000-4000-8000-000000000000" }),
        )
        .await
        .expect("the call itself succeeds");
    assert!(result.is_error);

    let body = harness::body(&result);
    let error = &body["error"];
    assert_eq!(error["kind"], json!("not_found"));
    assert_eq!(error["item_kind"], json!("component"));
    let candidates = error["candidates"].as_array().expect("candidates listed");
    assert!(
        candidates.len() >= 6,
        "the document's own symbol uuids are the hint: {candidates:?}"
    );
}

/// D.4.1.8's own check: an item whose *only* identity is a uuid — a junction
/// dot, a no-connect flag — has to be readable off a document the caller did
/// not just write, or its address exists and is unreachable.
///
/// Both are off by default in `get_schematic_layout`, so a caller that does
/// not need them pays nothing for them.
#[tokio::test]
async fn a_no_connect_read_back_from_a_layout_can_be_deleted_by_its_uuid() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    // Two items whose position is all they have, placed where nothing else is.
    let added = h
        .json(
            "add_no_connect",
            json!({ "schematic": sch, "x": 60.96, "y": 60.96 }),
        )
        .await;
    assert!(added.get("error").is_none(), "{added}");
    h.json(
        "add_junction",
        json!({ "schematic": sch, "x": 66.04, "y": 60.96 }),
    )
    .await;

    // The default summary says nothing about either.
    let plain = h
        .json("get_schematic_layout", json!({ "schematic": sch }))
        .await;
    assert!(plain.get("no_connects").is_none());
    assert!(plain.get("junctions").is_none());

    let layout = h
        .json(
            "get_schematic_layout",
            json!({
                "schematic": sch,
                "include_junctions": true,
                "include_no_connects": true
            }),
        )
        .await;
    assert_eq!(layout["junction_count"], json!(1));
    let no_connects = layout["no_connects"].as_array().expect("listed");
    assert_eq!(no_connects.len(), 1);
    let uuid = no_connects[0]["uuid"]
        .as_str()
        .expect("published")
        .to_string();

    let deleted = h
        .json(
            "delete_no_connect",
            json!({ "schematic": sch, "uuid": uuid }),
        )
        .await;
    assert!(deleted.get("error").is_none(), "{deleted}");

    let after = h
        .json(
            "get_schematic_layout",
            json!({ "schematic": sch, "include_no_connects": true, "include_junctions": true }),
        )
        .await;
    assert_eq!(after["no_connect_count"], json!(0));
    assert_eq!(
        after["junction_count"],
        json!(1),
        "deleting one point item leaves the other where it was"
    );
}

// ─── D.4.1.6: the plural forms ───────────────────────────────────────────────
//
// A tool that takes a list of addresses takes the second form the way its
// first one is already shaped: a parallel `uuids` array beside `references`,
// or a `uuid` field inside the entry objects it already reads. The two are
// accepted together and the batch is their union, each item acted on once.

/// A fresh copy of the divider, and its path as the tools take it.
async fn divider() -> (Harness, String) {
    let h = Harness::new();
    let path = h.repo_file(DIVIDER);
    let sch = path.display().to_string();
    (h, sch)
}

/// The document text, for comparing two runs that must land in the same place.
fn text(sch: &str) -> String {
    std::fs::read_to_string(sch).expect("the schematic is readable")
}

/// Same operation, two addresses, one document — for the parallel-array form.
#[tokio::test]
async fn bulk_move_and_batch_delete_agree_whichever_array_addresses_them() {
    for tool in [
        "bulk_move_schematic_components",
        "batch_delete_schematic_components",
    ] {
        let (by_ref, ref_sch) = divider().await;
        let (by_uuid, uuid_sch) = divider().await;
        let uuids = [
            component_uuid(&by_uuid, &uuid_sch, "R1").await,
            component_uuid(&by_uuid, &uuid_sch, "R2").await,
        ];

        let mut ref_args = json!({ "schematic": ref_sch, "references": ["R1", "R2"] });
        let mut uuid_args = json!({ "schematic": uuid_sch, "uuids": uuids });
        for args in [&mut ref_args, &mut uuid_args] {
            args["dx"] = json!(2.54);
            args["dy"] = json!(0.0);
        }
        let by_reference_result = by_ref.json(tool, ref_args).await;
        let by_uuid_result = by_uuid.json(tool, uuid_args).await;
        assert_eq!(
            by_reference_result["errors"],
            json!([]),
            "{tool} by reference: {by_reference_result}"
        );
        assert_eq!(
            by_uuid_result["errors"],
            json!([]),
            "{tool} by uuid: {by_uuid_result}"
        );
        assert_eq!(
            text(&ref_sch),
            text(&uuid_sch),
            "{tool} lands in the same document either way"
        );
    }
}

/// The same, for the two tools whose entries are objects: a `uuid` field
/// beside the address the entry already carried.
#[tokio::test]
async fn batch_edit_and_batch_rotate_agree_whichever_field_addresses_them() {
    let (by_ref, ref_sch) = divider().await;
    let (by_uuid, uuid_sch) = divider().await;
    let r1 = component_uuid(&by_uuid, &uuid_sch, "R1").await;

    by_ref
        .json(
            "batch_edit_schematic_components",
            json!({ "schematic": ref_sch, "edits": [{ "reference": "R1", "value": "4k7" }] }),
        )
        .await;
    by_uuid
        .json(
            "batch_edit_schematic_components",
            json!({ "schematic": uuid_sch, "edits": [{ "uuid": r1, "value": "4k7" }] }),
        )
        .await;
    assert_eq!(text(&ref_sch), text(&uuid_sch), "batch_edit by uuid");

    // The label's own anchor is what the uuid path writes back, so the two
    // rotations have to agree on it too.
    let label = by_uuid
        .json("list_schematic_labels", json!({ "schematic": uuid_sch }))
        .await["labels"]
        .as_array()
        .expect("listed")
        .iter()
        .find(|l| l["net"] == json!("VOUT"))
        .expect("the divider labels its midpoint")
        .clone();

    let rotated_by_net = by_ref
        .json(
            "batch_rotate_labels",
            json!({
                "schematic": ref_sch,
                "labels": [{ "net": "VOUT", "x": label["x"], "y": label["y"], "rotation": 90.0 }]
            }),
        )
        .await;
    let rotated_by_uuid = by_uuid
        .json(
            "batch_rotate_labels",
            json!({
                "schematic": uuid_sch,
                "labels": [{ "uuid": label["uuid"], "rotation": 90.0 }]
            }),
        )
        .await;
    assert_eq!(rotated_by_net["rotated"], json!(1), "{rotated_by_net}");
    assert_eq!(rotated_by_uuid["rotated"], json!(1), "{rotated_by_uuid}");
    assert_eq!(
        text(&ref_sch),
        text(&uuid_sch),
        "batch_rotate_labels by uuid"
    );
}

/// The read-only pair, which never writes: the pins answered are the same
/// pins, and the answer carries the designator either way.
#[tokio::test]
async fn pin_locations_and_grouping_take_the_uuids_array() {
    let (h, sch) = divider().await;
    let r1 = component_uuid(&h, &sch, "R1").await;
    let r2 = component_uuid(&h, &sch, "R2").await;

    let by_reference = h
        .json(
            "batch_get_schematic_pin_locations",
            json!({ "schematic": sch, "references": ["R1", "R2"] }),
        )
        .await;
    let by_uuid = h
        .json(
            "batch_get_schematic_pin_locations",
            json!({ "schematic": sch, "uuids": [r1.clone(), r2.clone()] }),
        )
        .await;
    assert_eq!(by_reference, by_uuid, "the uuids name the same two symbols");

    let (grouped_by_ref, ref_sch) = divider().await;
    let (grouped_by_uuid, uuid_sch) = divider().await;
    grouped_by_ref
        .json(
            "group_components",
            json!({ "schematic": ref_sch, "references": ["R1", "R2"], "group_name": "divider" }),
        )
        .await;
    grouped_by_uuid
        .json(
            "group_components",
            json!({ "schematic": uuid_sch, "uuids": [r1, r2], "group_name": "divider" }),
        )
        .await;
    assert_eq!(text(&ref_sch), text(&uuid_sch), "group_components by uuid");
}

/// Both forms in one call: the batch is their union, and the symbol named
/// twice moves once.
#[tokio::test]
async fn a_mixed_call_is_the_union_and_moves_each_symbol_once() {
    let (mixed_h, mixed_sch) = divider().await;
    let (plain_h, plain_sch) = divider().await;
    let r1 = component_uuid(&mixed_h, &mixed_sch, "R1").await;
    let r2 = component_uuid(&mixed_h, &mixed_sch, "R2").await;

    let mixed = mixed_h
        .json(
            "bulk_move_schematic_components",
            json!({
                "schematic": mixed_sch,
                "references": ["R1"],
                "uuids": [r1, r2],
                "dx": 2.54, "dy": 0.0
            }),
        )
        .await;
    assert_eq!(mixed["moved_count"], json!(2), "R1 once, R2 once: {mixed}");
    assert_eq!(mixed["errors"], json!([]));

    plain_h
        .json(
            "bulk_move_schematic_components",
            json!({ "schematic": plain_sch, "references": ["R1", "R2"], "dx": 2.54, "dy": 0.0 }),
        )
        .await;
    assert_eq!(
        text(&mixed_sch),
        text(&plain_sch),
        "R1 was named twice and moved by one offset, not two"
    );
}

/// A uuid that names an item of another kind is refused — and the rest of the
/// batch goes on, which is what this handler already does with a designator
/// that names nothing.
#[tokio::test]
async fn a_wrong_kind_uuid_is_refused_and_the_rest_of_the_batch_still_runs() {
    let (h, sch) = divider().await;
    let wire = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await["wires"]
        .as_array()
        .expect("listed")[0]["uuid"]
        .as_str()
        .expect("wires publish uuids")
        .to_string();
    let r2 = component_uuid(&h, &sch, "R2").await;

    let deleted = h
        .json(
            "batch_delete_schematic_components",
            json!({ "schematic": sch, "uuids": [wire.clone(), r2] }),
        )
        .await;
    assert_eq!(deleted["deleted"], json!(["R2"]), "{deleted}");
    let errors = deleted["errors"].as_array().expect("collected");
    assert_eq!(errors.len(), 1, "{deleted}");
    assert!(
        errors[0].as_str().unwrap().contains(&wire),
        "the refused address is named: {deleted}"
    );
    assert!(
        text(&sch).contains(&wire),
        "a wire uuid never deletes the wire through a component tool"
    );
}

/// Neither form is not an empty batch: it is a call with no address at all.
#[tokio::test]
async fn a_plural_call_with_no_address_is_an_invalid_argument() {
    let (h, sch) = divider().await;
    for tool in [
        "batch_delete_schematic_components",
        "bulk_move_schematic_components",
        "batch_get_schematic_pin_locations",
        "group_components",
    ] {
        let result = h
            .call(
                tool,
                json!({ "schematic": sch, "dx": 1.0, "dy": 1.0, "group_name": "g" }),
            )
            .await
            .expect("the call itself succeeds");
        assert!(result.is_error, "{tool} answered {:?}", result.content);
        assert_eq!(
            harness::body(&result)["error"]["kind"],
            json!("invalid_argument"),
            "{tool}"
        );
    }
}

/// A no-connect has no designator, so its entry takes the uuid the tool that
/// created it reported.
#[tokio::test]
async fn batch_delete_no_connect_takes_a_uuid_in_its_entry() {
    let (h, sch) = divider().await;
    let first = h
        .json(
            "add_no_connect",
            json!({ "schematic": sch, "x": 60.96, "y": 60.96 }),
        )
        .await["added_no_connect"]["uuid"]
        .as_str()
        .expect("add_no_connect reports the uuid it created")
        .to_string();
    h.json(
        "add_no_connect",
        json!({ "schematic": sch, "x": 66.04, "y": 66.04 }),
    )
    .await;

    let deleted = h
        .json(
            "batch_delete_no_connect",
            json!({ "schematic": sch, "positions": [{ "uuid": first }] }),
        )
        .await;
    assert_eq!(deleted["deleted"], json!(1), "{deleted}");
    assert!(
        !text(&sch).contains(&first),
        "the named flag is the one gone"
    );

    let left = h
        .json(
            "get_schematic_layout",
            json!({ "schematic": sch, "include_no_connects": true }),
        )
        .await;
    assert_eq!(left["no_connect_count"], json!(1), "the other one stayed");
}
