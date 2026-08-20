//! D.7.1's validation: a journal replay reconstructs the same semantic diff
//! the batch reported.
//!
//! `report_diff` (in `router::meta_tools`) computes the summary a
//! `kicad_invoke` reply carries by diffing the snapshot's before-image
//! against what landed on disk. This probe never reads that summary's inputs
//! directly — it goes through `RunJournal` exactly as a caller reconstructing
//! the change after the fact would: read the entry, pull the pre/post bytes
//! back out of the images the batch left behind, and recompute the diff from
//! those alone. If the two summaries agree, the journal really is a faithful
//! record of what the batch did, not just a note that it happened.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use harness::{as_str, body, Harness};
use kam_state::ImageSide;
use serde_json::json;
use std::path::Path;

const DIVIDER: &str = "bench/fixtures/divider.kicad_sch";

/// The uuid a reading tool publishes for `reference` — same helper as
/// `uuid_addressing.rs`, kept local since integration test binaries do not
/// share code across files.
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

#[tokio::test]
async fn a_journal_replay_reconstructs_the_batch_s_reported_diff() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);
    let uuid = component_uuid(&h, sch, "R1").await;

    let ctx = h.ctx();
    let result = konnect_core::router::meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({
            "calls": [{
                "tool": "move_schematic_component",
                "args": { "schematic": sch, "uuid": uuid, "x": 120.65, "y": 80.01 },
            }],
        }),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta-tool");
    assert!(!result.is_error, "{result:?}");
    let reply = body(&result);
    let reported_summary = reply["diff"]["summary"]
        .as_str()
        .expect("a move is reported with a diff summary")
        .to_string();

    let journal = ctx
        .journal
        .as_ref()
        .expect("this harness enabled a journal");
    let entries = journal.entries().expect("journal reads back");
    let entry = entries.last().expect("the batch wrote one entry");
    assert_eq!(entry.outcome, kam_state::Outcome::Applied);
    assert_eq!(entry.documents.len(), 1, "only the schematic moved");

    let pre = journal
        .image(entry, 0, ImageSide::Pre)
        .expect("the pre-image is still on disk");
    let post = journal
        .image(entry, 0, ImageSide::Post)
        .expect("the post-image is still on disk");

    // The relative path the journal stored already carries the right
    // extension for `diff_document`'s dispatch — reusing it is what makes
    // this a replay from the journal alone, not a re-derivation from the
    // fixture path the test already knows.
    let doc_path = Path::new(&entry.documents[0].path);
    let diff = konnect_core::evidence::diff_document(doc_path, Some(&pre), Some(&post))
        .expect("both images parse as a schematic");
    assert_eq!(
        diff.summary(),
        reported_summary,
        "the diff recomputed from the journal's images must match what the batch reported"
    );
}

/// A batch that only reads is not an event this journal exists to record —
/// see `router::meta_tools::BatchGuard::finish`, which never reaches the
/// journal write for an empty `changed()`.
#[tokio::test]
async fn a_read_only_batch_writes_nothing_to_the_journal() {
    let h = Harness::with_journal();
    let sch = h.repo_file(DIVIDER);
    let sch = as_str(&sch);

    let ctx = h.ctx();
    let result = konnect_core::router::meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({
            "calls": [{
                "tool": "list_schematic_components",
                "args": { "schematic": sch },
            }],
        }),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta-tool");
    assert!(!result.is_error, "{result:?}");

    let journal = ctx
        .journal
        .as_ref()
        .expect("this harness enabled a journal");
    assert!(
        journal.entries().expect("journal reads back").is_empty(),
        "a read-only batch must leave no trace in the run journal"
    );
}
