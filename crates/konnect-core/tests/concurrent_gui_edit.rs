//! L.2.3 — a GUI holding the same file open is outside the file-level
//! rollback (D12: rollback restores *this process's* before-image, it cannot
//! reach edits a different application makes on disk). `base_revisions`
//! (checked once, before the batch starts — meta_tools.rs `handle_kicad_invoke`)
//! cannot see it either: it only catches a stale start, not a write landing
//! *during* the batch. What actually catches a mid-batch GUI save is the
//! per-write compare-and-swap in `write_atomic_if_unchanged`
//! (konnect-sexp/src/writer.rs): every schematic tool does
//! `read_consistent` → compute → `write_atomic_if_unchanged(path, expected, new)`,
//! and a GUI save landing between those two steps makes `expected` stale, so
//! the write is refused with `SexpError::Conflict` instead of silently
//! clobbering what the GUI wrote.
//!
//! Why a real writer and not a mock: `write_atomic_if_unchanged`'s guarantee
//! is specifically that it catches "applications that do not honor the
//! advisory lock, including KiCad itself" (writer.rs doc comment) — a real
//! writer thread that never opens Konnect's lock file is the closest honest
//! stand-in for that GUI, on every OS this suite runs on. That writer must
//! itself write atomically (temp file in the same directory, then
//! `std::fs::rename` over the target — exactly how KiCad and every other
//! well-behaved editor saves): a naive `std::fs::write` truncates the target
//! before writing its new bytes, opening a window where the file is
//! momentarily empty. `read_consistent` landing in that window sees a torn,
//! zero-byte read — not a *modified* file — which surfaces as a parse error
//! (`handler_error`) instead of the `conflict` this test exists to prove.
//! Atomic rename removes that parasitic window entirely: every reader either
//! sees the file before or after a GUI save, in full, never in between.
//!
//! Making the race deterministic without touching production code: the GUI
//! thread writes in a tight loop for as long as the batch runs, so across
//! however many `read_consistent`/`write_atomic_if_unchanged` windows the
//! batch opens, at least one GUI write is virtually certain to land inside
//! one of them. There is no sleep-based timing dependency — the test's
//! correctness does not rely on any particular write landing at any
//! particular moment, only on enough of them happening for the race to be
//! exercised at least once, which a tight loop for the batch's whole
//! duration make near-certain in practice; the assertions below hold
//! regardless of exactly which call the conflict lands on.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::{meta_tools, ToolRouter};
use konnect_core::tools::{ServerConfig, ToolContext};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn context() -> Arc<ToolContext> {
    let router = Arc::new(ToolRouter::new());
    Arc::new(ToolContext::new(
        ServerConfig {
            kicad_cli: String::new(),
            kicad_binary: String::new(),
            ipc_address: String::new(),
            project_dir: None,
            jlcpcb_db_path: None,
            auto_load_toolsets: false,
            mode: kam_state::OperatingMode::Write,
        },
        router,
    ))
}

fn body(result: &CallToolResult) -> Value {
    let ToolContent::Text { text } = &result.content[0] else {
        panic!("structured results are text content");
    };
    serde_json::from_str(text).expect("result body is JSON")
}

fn label_call(sch: &std::path::Path, n: usize) -> Value {
    json!({
        "tool": "add_schematic_net_label",
        "args": {
            "schematic": sch,
            "net": format!("N{n}"),
            "x": 100.33 + (n as f64) * 2.54,
            "y": 87.63,
        }
    })
}

/// A GUI save: write-to-temp-then-rename, never touching Konnect's advisory
/// lock file, appending a growing, uniquely-marked trailing comment so every
/// write is both distinct and trivially recognizable in the final content.
/// The rename is atomic on every OS this suite runs on, so a reader either
/// sees the whole file before this save or the whole file after it — never a
/// torn, truncated read — which is what lets a failure of this test's core
/// assertion mean "the compare-and-swap decayed", not "the GUI stand-in
/// tore a read".
fn spawn_gui_writer(
    path: std::path::PathBuf,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<usize> {
    std::thread::spawn(move || {
        let dir = path
            .parent()
            .expect("schematic path has a parent dir")
            .to_path_buf();
        let tmp = dir.join(".gui-save.tmp");
        let mut writes = 0usize;
        while !stop.load(Ordering::Relaxed) {
            let Ok(current) = std::fs::read_to_string(&path) else {
                continue;
            };
            // A trailing blank line is inert to every text-based block finder
            // in konnect-sexp (they scan for `(tag`, never anchor on EOF), so
            // this is a content change that cannot itself break parsing.
            let edited = format!("{current}\n; gui-save-{writes}\n");
            if std::fs::write(&tmp, edited).is_err() {
                continue;
            }
            // On Windows, `rename` fails if the target is open elsewhere
            // without `FILE_SHARE_DELETE`; treat that as an uncounted miss
            // and keep looping rather than panicking, so the race stays
            // exercised even if some attempts lose to a held handle.
            if std::fs::rename(&tmp, &path).is_ok() {
                writes += 1;
            }
        }
        let _ = std::fs::remove_file(&tmp);
        writes
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_gui_save_racing_a_batch_is_caught_by_the_per_write_compare_and_swap_not_lost_silently() {
    let ctx = context();
    let dir = tempfile::tempdir().expect("tempdir");

    let created = meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({"calls": [
            {"tool": "create_project", "args": {"path": dir.path(), "name": "gui_race"}}
        ]}),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta tool");
    let created = body(&created);
    assert_eq!(created["ok"].as_u64(), Some(1), "{created:#?}");

    let sch = dir.path().join("gui_race.kicad_sch");
    let pre_batch_content = std::fs::read_to_string(&sch).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let gui = spawn_gui_writer(sch.clone(), stop.clone());

    // atomic defaults to stop_on_error, which defaults to true: this batch is
    // one unit of work, so a mid-batch conflict must stop it and roll back.
    let calls: Vec<Value> = (0..300).map(|n| label_call(&sch, n)).collect();
    let result = meta_tools::handle_meta_tool("kicad_invoke", &json!({"calls": calls}), &ctx)
        .await
        .expect("kicad_invoke is a meta tool");

    stop.store(true, Ordering::Relaxed);
    let gui_writes = gui.join().expect("gui writer thread did not panic");
    assert!(
        gui_writes > 0,
        "gui writer never got a write in — race setup is broken"
    );

    let result_body = body(&result);
    // The envelope itself is never `is_error` (meta_tools.rs: "the envelope
    // itself succeeded even when an inner call did not") — the batch's own
    // failure shows up as `failed_at`, not as a protocol-level error.
    assert!(
        result_body.get("failed_at").is_some(),
        "a batch racing a GUI save for 300 sequential read/write windows must hit at \
         least one conflict: {result_body:#?}"
    );

    // The failing call must be classified `conflict` / `state`: the world
    // moved under the batch, so the only useful response is re-read and
    // recompute, never a blind retry.
    let results = result_body["results"].as_array().expect("results array");
    let failure = results
        .iter()
        .find(|entry| entry["ok"] == json!(false))
        .unwrap_or_else(|| panic!("no failing call recorded: {result_body:#?}"));
    assert_eq!(
        failure["error_kind"], "conflict",
        "the write conflict must not have decayed into handler_error: {failure:#?}"
    );
    assert_eq!(
        failure["transient"], "state",
        "a conflict must tell the caller to re-read and recompute, not retry blind: {failure:#?}"
    );

    // Rollback (atomic, default true) restores this process's own
    // before-image. It cannot special-case "but a GUI also wrote here" — its
    // only promise is that Konnect's own half-applied batch does not survive.
    // What matters for the GUI-edit guarantee is the other half: the GUI's
    // write was never silently clobbered *by a batch write believing it had
    // the current content* — that write was refused before it happened. The
    // final file is therefore always one coherent version — either the
    // pre-batch snapshot restored by rollback, or the GUI's own last save —
    // never a torn mix of the two, and never the batch's edits sitting
    // silently on top of a discarded GUI save.
    let after = std::fs::read_to_string(&sch).unwrap();
    let is_pristine_rollback = after == pre_batch_content;
    let is_gui_last_write = after.contains("; gui-save-");
    assert!(
        is_pristine_rollback || is_gui_last_write,
        "final file is neither the rolled-back original nor a GUI save — a torn state: \
         {after}"
    );

    // Whichever it is, no add_schematic_net_label content the batch produced
    // survives past a conflict-triggered rollback.
    for n in 0..300 {
        assert!(
            !after.contains(&format!("N{n}")),
            "batch label N{n} survived a rollback that should have undone it"
        );
    }

    // Recovery loop. Every schematic tool call does `read_consistent` fresh
    // before it writes (this is not special to the batch path), so at this
    // layer "replay blindly" and "re-read, then replay" are the same call —
    // there is no separate stale snapshot to carry across retries the way
    // `base_revisions` carries one across a whole batch. What actually
    // separates the two outcomes the task asks for is whether the world is
    // still moving: replayed while a GUI keeps racing, the identical call can
    // still lose the same race and fail again; replayed once the world has
    // settled, that same fresh read makes it succeed. A failed call never
    // writes, so looping the identical call is safe — nothing to duplicate.
    let stop2 = Arc::new(AtomicBool::new(false));
    let gui2 = spawn_gui_writer(sch.clone(), stop2.clone());
    let recovery_call = label_call(&sch, 9999);

    let mut saw_conflict_while_racing = false;
    for _ in 0..500 {
        let retry = meta_tools::handle_meta_tool(
            "kicad_invoke",
            &json!({"calls": [recovery_call.clone()]}),
            &ctx,
        )
        .await
        .expect("kicad_invoke is a meta tool");
        let retry_body = body(&retry);
        if retry_body.get("failed_at").is_some() {
            let results = retry_body["results"].as_array().unwrap();
            assert_eq!(
                results[0]["error_kind"], "conflict",
                "the only failure mode racing a GUI save should produce: {retry_body:#?}"
            );
            saw_conflict_while_racing = true;
            break;
        }
    }
    stop2.store(true, Ordering::Relaxed);
    gui2.join().expect("gui writer thread did not panic");
    assert!(
        saw_conflict_while_racing,
        "recovery-loop setup is broken: 500 identical replays never lost the race \
         to an actively-writing GUI"
    );

    // Same call, resubmitted once the GUI has stopped: its own fresh read
    // now sees a settled file, and it succeeds.
    let recovered =
        meta_tools::handle_meta_tool("kicad_invoke", &json!({"calls": [recovery_call]}), &ctx)
            .await
            .expect("kicad_invoke is a meta tool");
    let recovered_body = body(&recovered);
    assert!(
        recovered_body.get("failed_at").is_none(),
        "the identical call must succeed once nothing is racing it: {recovered_body:#?}"
    );
    assert_eq!(recovered_body["ok"].as_u64(), Some(1));
}

// Negative control (run manually, not part of the suite): with both
// compare-and-swap checks in `write_atomic_if_unchanged` short-circuited to
// `false`, the batch above no longer sees `conflict` — it instead corrupts
// the file (a GUI write landing mid-write splices its bytes into what the
// batch was writing) and the *next* call fails to parse it, surfacing as
// `handler_error` with a parse error instead of `conflict`. Either way the
// "must hit at least one conflict" assertion fails, confirming both checks
// are load-bearing, not vacuous — and that disabling either one trades a
// clean refusal for silent corruption, not for a difference in bookkeeping.
