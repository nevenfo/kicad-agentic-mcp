//! W.1 — a schematic KiCad has open is not writable, and the refusal is
//! something a client can act on.
//!
//! # The window this closes
//!
//! `write_atomic_if_unchanged`'s compare-and-swap (proved by
//! `concurrent_gui_edit.rs`) catches a GUI that *already saved*: the bytes on
//! disk moved, so `expected` is stale and the write is refused. It cannot
//! catch the case that actually loses work, because that one leaves no trace
//! on disk at all:
//!
//! 1. Eeschema opens `design.kicad_sch`;
//! 2. the user edits, and the newer document exists only in the editor's
//!    memory — the file on disk is untouched, so every content check passes;
//! 3. Konnect writes, correctly, against the unchanged file;
//! 4. the user hits Ctrl+S, and Eeschema writes its in-memory document over
//!    Konnect's change, which is gone with no error anywhere.
//!
//! The only evidence available *before* step 4 is the sibling lock KiCad drops
//! when it opens a document. Probed against a real KiCad 10 on the supported
//! platform rather than taken from a description: `eeschema.exe` opening
//! `X.kicad_sch` creates `~X.kicad_sch.lck` next to it (alongside
//! `~X.kicad_pro.lck`), 50 bytes, holding exactly
//! `{"hostname":"…","username":"…"}`; a clean close removes both.
//!
//! # Why the lock is never judged
//!
//! That file carries no pid, no process start time, and no document token.
//! Nothing in it can tell a live editor from one a crash left behind — still
//! less for a lock written by another host onto a network share. So Konnect
//! does not parse it, does not score it, and never removes it: presence is
//! refusal, and resolving a lock stays a decision a human makes after checking
//! that no editor owns the file.
//!
//! What this file proves, that the `konnect-sexp` unit tests do not: the
//! refusal survives the whole tool path, reaching a caller as the catalogued
//! `conflict` kind with the blocked path in it, both for a single
//! `tools/call` and for a `kicad_invoke` batch entry.

mod harness;

use harness::Harness;
use konnect_core::mcp::error::ToolErrorKind;
use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::meta_tools;
use serde_json::{json, Value};

/// KiCad's own lock file for `schematic`, with the exact contents a real
/// KiCad 10 session was observed to write.
fn plant_editor_lock(schematic: &std::path::Path) -> std::path::PathBuf {
    let name = schematic
        .file_name()
        .expect("the fixture path names a file");
    let lock = schematic.with_file_name(format!("~{}.lck", name.to_string_lossy()));
    std::fs::write(
        &lock,
        r#"{"hostname":"DESKTOP-0JR01AG","username":"FlowUP"}"#,
    )
    .expect("the lock is writable");
    lock
}

/// Sorted names of everything in `schematic`'s directory — the oracle for
/// "the refusal created nothing".
fn directory_entries(schematic: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(schematic.parent().expect("a parent directory"))
        .expect("the directory is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn body(result: &CallToolResult) -> Value {
    let ToolContent::Text { text } = result.content.first().expect("a text block") else {
        panic!("structured results are text content");
    };
    serde_json::from_str(text).expect("the result body is JSON")
}

/// Every write tool refuses while the lock exists, and the document, the lock
/// and the directory around them are exactly as they were.
#[tokio::test]
async fn a_locked_schematic_refuses_every_write_and_leaves_nothing_behind() {
    let h = Harness::new();
    let sch = h.fixture(harness::TWO_RESISTORS);
    let lock = plant_editor_lock(&sch);

    let before = std::fs::read(&sch).expect("the fixture is readable");
    let lock_before = std::fs::read(&lock).expect("the lock is readable");
    let entries_before = directory_entries(&sch);

    // Three different write paths, deliberately: a wire insertion, a property
    // edit, and a component placement each reach the shared writer through a
    // different handler, and a guard that only covered one of them would pass
    // a narrower test than this.
    let writes = [
        (
            "add_wire",
            json!({"schematic": harness::as_str(&sch), "x1": 60.0, "y1": 60.0, "x2": 70.0, "y2": 60.0}),
        ),
        (
            "edit_schematic_component",
            json!({"schematic": harness::as_str(&sch), "reference": "R1", "value": "22k"}),
        ),
        (
            "add_schematic_net_label",
            json!({"schematic": harness::as_str(&sch), "net": "VCC", "x": 101.6, "y": 45.72}),
        ),
    ];

    for (tool, args) in &writes {
        let error = h
            .call(tool, args.clone())
            .await
            .err()
            .unwrap_or_else(|| panic!("'{tool}' must refuse a locked schematic"));

        let kind = ToolErrorKind::from_anyhow(&error);
        assert_eq!(
            kind.short_code(),
            "conflict",
            "'{tool}' must refuse as a catalogued conflict, not as handler_error: {error}"
        );
        match &kind {
            ToolErrorKind::Conflict { path } => assert!(
                path.ends_with(harness::TWO_RESISTORS),
                "the refusal must name the blocked schematic, got {path}"
            ),
            other => panic!("'{tool}': unexpected kind {other:?}"),
        }
        assert!(
            error.to_string().contains(".lck"),
            "'{tool}': the message must name the lock file so a human knows what to close: {error}"
        );
    }

    assert_eq!(
        std::fs::read(&sch).expect("the fixture is readable"),
        before,
        "a refused write must leave the schematic bit-identical"
    );
    assert_eq!(
        std::fs::read(&lock).expect("the lock is readable"),
        lock_before,
        "Konnect must never rewrite a KiCad lock"
    );
    assert_eq!(
        directory_entries(&sch),
        entries_before,
        "a refused write must create no scratch file and no transaction journal"
    );
}

/// Reads keep working while the lock exists. A locked document that could not
/// even be inspected would make the refusal impossible to understand.
#[tokio::test]
async fn a_locked_schematic_is_still_readable() {
    let h = Harness::new();
    let sch = h.fixture(harness::TWO_RESISTORS);
    plant_editor_lock(&sch);

    let listed = h
        .json(
            "list_schematic_components",
            json!({"schematic": harness::as_str(&sch)}),
        )
        .await;
    let references: Vec<&str> = listed["components"]
        .as_array()
        .expect("components is an array")
        .iter()
        .filter_map(|c| c["reference"].as_str())
        .collect();
    assert!(
        references.contains(&"R1") && references.contains(&"R2"),
        "a locked schematic must stay readable: {listed}"
    );
}

/// Closing the editor removes the lock, and the identical call then succeeds
/// and is visible to an independent re-read. Without this, "refuses
/// everything" would satisfy every other assertion in this file.
#[tokio::test]
async fn the_identical_write_succeeds_once_the_editor_closes() {
    let h = Harness::new();
    let sch = h.fixture(harness::TWO_RESISTORS);
    let lock = plant_editor_lock(&sch);
    let args = json!({"schematic": harness::as_str(&sch), "reference": "R1", "value": "22k"});

    assert!(
        h.call("edit_schematic_component", args.clone())
            .await
            .is_err(),
        "the write must be refused while the lock exists"
    );

    // What a clean Eeschema close does, and the only thing that is allowed to:
    // KiCad removes its own lock.
    std::fs::remove_file(&lock).expect("the lock is removable");

    let applied = h.json("edit_schematic_component", args).await;
    assert_eq!(applied["reference"], "R1", "{applied}");

    // Independent re-read, through a different tool than the one that wrote:
    // the change is on disk, not only in the writer's answer.
    let reread = h
        .json(
            "get_schematic_component",
            json!({"schematic": harness::as_str(&sch), "reference": "R1"}),
        )
        .await;
    assert_eq!(
        reread["value"], "22k",
        "the edit must survive to an independent read: {reread}"
    );
}

/// A lock that appears *after* a batch has started still stops it, and the
/// batch rolls back rather than leaving half its work behind.
///
/// This is the guard's real claim: it is not a one-off check at the top of an
/// operation, it is re-evaluated at every write, including the last one before
/// a document is replaced. Made deterministic without touching production
/// code — the batch's first call runs against no lock, the lock is planted,
/// and the second call meets it.
#[tokio::test]
async fn a_lock_appearing_mid_batch_stops_it_and_rolls_it_back() {
    let h = Harness::new();
    let sch = h.fixture(harness::TWO_RESISTORS);
    let ctx = h.ctx();
    let before = std::fs::read_to_string(&sch).expect("the fixture is readable");

    let first = meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({"calls": [{
            "tool": "add_schematic_net_label",
            "args": {"schematic": harness::as_str(&sch), "net": "EARLY", "x": 101.6, "y": 45.72}
        }]}),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta tool");
    assert_eq!(body(&first)["ok"].as_u64(), Some(1), "{:#?}", body(&first));

    let lock = plant_editor_lock(&sch);
    let after_first = std::fs::read_to_string(&sch).expect("the fixture is readable");
    let entries_before = directory_entries(&sch);

    let blocked = meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({"calls": [{
            "tool": "add_schematic_net_label",
            "args": {"schematic": harness::as_str(&sch), "net": "LATE", "x": 101.6, "y": 43.18}
        }]}),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta tool");
    let blocked = body(&blocked);

    assert!(
        blocked.get("failed_at").is_some(),
        "a batch touching a locked schematic must fail: {blocked:#?}"
    );
    let failure = blocked["results"]
        .as_array()
        .expect("results array")
        .iter()
        .find(|entry| entry["ok"] == json!(false))
        .unwrap_or_else(|| panic!("no failing call recorded: {blocked:#?}"));
    assert_eq!(
        failure["error_kind"], "conflict",
        "the batch path must classify an editor lock the same way: {failure:#?}"
    );

    assert_eq!(
        std::fs::read_to_string(&sch).expect("the fixture is readable"),
        after_first,
        "the blocked batch must leave the schematic exactly as the first batch left it"
    );
    assert!(
        !std::fs::read_to_string(&sch).unwrap().contains("LATE"),
        "no part of the blocked batch may reach disk"
    );
    assert_ne!(
        std::fs::read_to_string(&sch).unwrap(),
        before,
        "the earlier, unlocked batch must still be there — rollback undoes the \
         blocked batch, not the project's history"
    );
    assert_eq!(
        directory_entries(&sch),
        entries_before,
        "the blocked batch must leave no scratch file and no transaction journal"
    );
    assert!(lock.exists(), "Konnect must never remove a KiCad lock");
}
