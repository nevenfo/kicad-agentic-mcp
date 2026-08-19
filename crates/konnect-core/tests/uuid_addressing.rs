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
