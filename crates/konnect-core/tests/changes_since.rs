//! D.7.2's probe: `changes_since` reads back what the run journal already
//! recorded for a document, against a project fixture rather than a
//! hand-written one — see `run_journal.rs` for the same choice and why.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use harness::{as_str, body, Harness};
use kam_state::DocState;
use serde_json::json;

const DIVIDER: &str = "bench/fixtures/divider.kicad_sch";

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

async fn move_r1(h: &Harness, sch: &str, x: f64, y: f64) -> serde_json::Value {
    let uuid = component_uuid(h, sch, "R1").await;
    let ctx = h.ctx();
    let result = konnect_core::router::meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({
            "calls": [{
                "tool": "move_schematic_component",
                "args": { "schematic": sch, "uuid": uuid, "x": x, "y": y },
            }],
        }),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta-tool");
    assert!(!result.is_error, "{result:?}");
    body(&result)
}

async fn changes_since(h: &Harness, document: &str, since: &str) -> serde_json::Value {
    let ctx = h.ctx();
    let result = konnect_core::router::meta_tools::handle_meta_tool(
        "changes_since",
        &json!({ "document": document, "since": since }),
        &ctx,
    )
    .await
    .expect("changes_since is a meta-tool");
    assert!(!result.is_error, "{result:?}");
    body(&result)
}

/// 1: a batch mutates the document; `changes_since` from the revision before
/// it sees exactly one change, whose summary matches what the batch itself
/// reported.
#[tokio::test]
async fn a_mutating_batch_shows_up_as_one_change() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    let rev_before = DocState::read(std::path::Path::new(sch)).unwrap().token();

    let reply = move_r1(&h, sch, 120.65, 80.01).await;
    let reported_summary = reply["diff"]["summary"]
        .as_str()
        .expect("a move is reported with a diff summary")
        .to_string();

    let out = changes_since(&h, sch, &rev_before).await;
    assert_eq!(out["up_to_date"], json!(false));
    assert_eq!(out["foreign_edit"], json!(false));
    assert_eq!(out["since_known"], json!(true));
    assert_eq!(out["journal"], json!(true));
    let changes = out["changes"].as_array().expect("changes is an array");
    assert_eq!(changes.len(), 1, "{out}");
    assert_eq!(changes[0]["summary"], json!(reported_summary));
    assert_eq!(changes[0]["outcome"], json!("applied"));
}

/// 2: called with the revision the document is already at, `up_to_date` is
/// true and there is nothing new to report.
#[tokio::test]
async fn the_current_revision_is_up_to_date_with_no_changes() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    move_r1(&h, sch, 120.65, 80.01).await;

    let current = DocState::read(std::path::Path::new(sch)).unwrap().token();
    let out = changes_since(&h, sch, &current).await;
    assert_eq!(out["up_to_date"], json!(true));
    assert_eq!(out["changes"], json!([]));
}

/// 3: a write that did not go through Konnect moves the file's revision
/// without adding a journal entry — `foreign_edit` must catch that.
#[tokio::test]
async fn a_write_outside_konnect_is_a_foreign_edit() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    move_r1(&h, sch, 120.65, 80.01).await;

    let after_batch = DocState::read(std::path::Path::new(sch)).unwrap().token();
    let mut bytes = std::fs::read(sch).unwrap();
    bytes.extend_from_slice(b"\n; foreign edit\n");
    std::fs::write(sch, bytes).unwrap();

    let out = changes_since(&h, sch, &after_batch).await;
    assert_eq!(out["foreign_edit"], json!(true), "{out}");
}

/// 4: a `since` the journal never saw is reported, not refused.
#[tokio::test]
async fn an_unknown_since_is_reported_not_refused() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    move_r1(&h, sch, 120.65, 80.01).await;

    let out = changes_since(&h, sch, "0000000000000000-0").await;
    assert_eq!(out["since_known"], json!(false), "{out}");
}

/// 5: no journal at all — the call still answers, it just cannot say
/// anything about history.
#[tokio::test]
async fn no_journal_still_answers_with_journal_false() {
    let h = Harness::new();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    let rev = DocState::read(std::path::Path::new(sch)).unwrap().token();

    let ctx = h.ctx();
    let result = konnect_core::router::meta_tools::handle_meta_tool(
        "changes_since",
        &json!({ "document": sch, "since": rev }),
        &ctx,
    )
    .await
    .expect("changes_since is a meta-tool");
    assert!(!result.is_error, "{result:?}");
    let out = body(&result);
    assert_eq!(out["journal"], json!(false));
    assert_eq!(out["up_to_date"], json!(true));
    assert_eq!(out["changes"], json!([]));
}

/// 6: `document` or `since` missing is refused as `invalid_argument`.
#[tokio::test]
async fn missing_document_or_since_is_refused() {
    let h = Harness::with_journal();
    let ctx = h.ctx();

    let missing_document = konnect_core::router::meta_tools::handle_meta_tool(
        "changes_since",
        &json!({ "since": "abc-1" }),
        &ctx,
    )
    .await
    .expect("changes_since is a meta-tool");
    assert!(missing_document.is_error);

    let missing_since = konnect_core::router::meta_tools::handle_meta_tool(
        "changes_since",
        &json!({ "document": "a.kicad_sch" }),
        &ctx,
    )
    .await
    .expect("changes_since is a meta-tool");
    assert!(missing_since.is_error);
}
