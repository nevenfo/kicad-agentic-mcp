//! L.2.2 — the `lock` transient class promises two things: a call that loses
//! the race to an identical `operation_id` gets `operation_in_flight` /
//! `transient: lock` / `retry_after_ms: 250`, and the same call retried once
//! the winner finishes recovers (or gets the memoized result) without
//! double-applying. Both halves need a *real* concurrent race — the
//! sequential replay in `kicad_invoke_replays_an_operation_id_instead_of_applying_it_twice`
//! (protocol_stdio.rs) proves idempotency, not the race.
//!
//! Why this lives here and not in `crates/konnect/tests/`: the stdio
//! transport (`konnect::transport::stdio::run_stdio`) reads one JSON-RPC line,
//! awaits it to completion, and only then reads the next — by construction, a
//! single spawned `konnect` process can never have two `kicad_invoke` calls in
//! flight at once. Two separate processes would not race either: the
//! idempotency ledger lives in one process's `ToolContext`, in memory, not
//! shared across processes. A genuine race needs two tasks sharing one
//! `ToolContext`, which means driving `konnect_core::router::meta_tools`
//! directly, the way `konnect-core/tests/integration_test.rs` already does.
//!
//! Making the race deterministic without touching production code: every
//! `add_schematic_net_label` call inside the batch is a separate, real
//! `overwrite()` of the schematic file, so the file's size grows once per
//! call. The first task's batch is deliberately long (200 calls); the second
//! task only fires once it has observed the file grow past its original
//! size — proof, from the outside, that the first task is already past its
//! `claim()` and deep inside its loop, with roughly 199 calls still to run
//! before it reaches `complete()`. That margin, not timing luck, is what
//! makes the second call reliably lose the race.

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::{meta_tools, ToolRouter};
use konnect_core::tools::{ServerConfig, ToolContext};
use serde_json::{json, Value};
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

/// A batch call for `add_schematic_net_label` at a distinct, harmless grid
/// point, so `n` calls never collide with each other on disk position.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_call_racing_an_in_flight_operation_id_gets_lock_and_the_winner_is_not_double_applied(
) {
    let ctx = context();
    let dir = tempfile::tempdir().expect("tempdir");

    let created = meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({"calls": [
            {"tool": "create_project", "args": {"path": dir.path(), "name": "race"}}
        ]}),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta tool");
    let created = body(&created);
    assert_eq!(created["ok"].as_u64(), Some(1), "{created:#?}");

    let sch = dir.path().join("race.kicad_sch");
    let baseline_len = std::fs::metadata(&sch).unwrap().len();

    let calls: Vec<Value> = (0..200).map(|n| label_call(&sch, n)).collect();
    let winner_args = json!({"operation_id": "op_race", "calls": calls});

    let winner_ctx = ctx.clone();
    let winner_args_task = winner_args.clone();
    let winner = tokio::spawn(async move {
        meta_tools::handle_meta_tool("kicad_invoke", &winner_args_task, &winner_ctx)
            .await
            .expect("kicad_invoke is a meta tool")
    });

    // Wait for external, observable proof that the winner is mid-batch: the
    // file has grown past its size right after `create_project`. This is not
    // a sleep-and-hope — it is evidence that `claim()` already ran (it is the
    // first thing `handle_kicad_invoke` does after the synchronous
    // `base_revisions` check) and that ~199 of 200 calls remain before
    // `complete()`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let len = std::fs::metadata(&sch).map(|m| m.len()).unwrap_or(0);
        if len > baseline_len {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the winner never started writing within 10s — race setup is broken, not just slow"
        );
        tokio::task::yield_now().await;
    }

    // The loser: same operation_id, issued while the winner is provably still
    // running. It must not touch the file, and must say so as `lock`.
    let loser = meta_tools::handle_meta_tool(
        "kicad_invoke",
        &json!({"operation_id": "op_race", "calls": [label_call(&sch, 9000)]}),
        &ctx,
    )
    .await
    .expect("kicad_invoke is a meta tool");
    assert!(loser.is_error, "{:#?}", body(&loser));
    let loser_body = body(&loser);
    assert_eq!(
        loser_body["error"]["kind"], "operation_in_flight",
        "{loser_body:#?}"
    );
    assert_eq!(loser_body["error"]["operation_id"], "op_race");
    assert_eq!(loser_body["error"]["transient"], "lock");
    assert_eq!(loser_body["error"]["retry_after_ms"], 250);

    let winner_result = winner.await.expect("winner task did not panic");
    let winner_body = body(&winner_result);
    assert_eq!(
        winner_body["ok"].as_u64(),
        Some(200),
        "the winner must have completed all 200 calls unharmed by the race: {winner_body:#?}"
    );
    let after_winner = std::fs::read_to_string(&sch).unwrap();

    // Recovery: replay the identical operation_id/batch now that the winner
    // is done. It must not re-apply — the memoized result comes back, and the
    // file is untouched by this call.
    let replay = meta_tools::handle_meta_tool("kicad_invoke", &winner_args, &ctx)
        .await
        .expect("kicad_invoke is a meta tool");
    let replay_body = body(&replay);
    assert_eq!(replay_body["replayed"], json!(true), "{replay_body:#?}");
    assert_eq!(replay_body["ok"].as_u64(), Some(200));
    assert_eq!(
        std::fs::read_to_string(&sch).unwrap(),
        after_winner,
        "the replay must not have added a second set of labels"
    );
}
