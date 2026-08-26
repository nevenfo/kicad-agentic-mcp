//! P.6.9.11: a path argument that was never supplied is an `invalid_argument`,
//! whichever helper the handler happened to reach for.
//!
//! `require_str` returns a structured `InvalidArgument`; `get_path` returns an
//! `anyhow::Error` that the dispatch stringified into `handler_error`. Which
//! vocabulary a caller got therefore depended on the helper the handler was
//! written with, which is arbitrary. The classification now travels in the
//! error chain as a type and is downcast where the anyhow error is turned into
//! a `ToolErrorKind`.
//!
//! P.6.9.10 made the dispatch enforce every schema's `required` list, so a
//! `tools/call` for a tool whose path key is `required` no longer reaches
//! `get_path` with the key absent. The gateway is the surface that still does:
//! `kicad_invoke` calls `(def.handler)` per entry without that check (see
//! `router::meta_tools::handle_kicad_invoke`), by design — the entry's own
//! required list is the gateway's business. So the entries below are where the
//! two vocabularies are observable side by side.

mod harness;

use konnect_core::mcp::handler::McpHandler;
use konnect_core::tools::ServerConfig;
use serde_json::{json, Value};
use std::path::Path;

async fn handler_at(project_dir: &Path) -> McpHandler {
    let config = ServerConfig {
        kicad_cli: String::new(),
        kicad_binary: String::new(),
        ipc_address: String::new(),
        project_dir: Some(project_dir.to_path_buf()),
        jlcpcb_db_path: None,
        auto_load_toolsets: false,
        mode: kam_state::OperatingMode::Write,
    };
    McpHandler::new(config).await.expect("handler constructs")
}

async fn call(handler: &McpHandler, name: &str, arguments: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let resp = handler
        .handle_message(req)
        .await
        .expect("tools/call always answers");
    serde_json::to_value(resp).expect("response serializes")
}

fn tool_body(resp: &Value) -> Value {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("tool result carries text content");
    serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
}

/// One `kicad_invoke` entry, and the entry's own answer.
async fn invoke_entry(handler: &McpHandler, tool: &str, args: Value) -> Value {
    let resp = call(
        handler,
        "kicad_invoke",
        json!({ "calls": [ { "tool": tool, "args": args } ] }),
    )
    .await;
    tool_body(&resp)["results"][0].clone()
}

/// `list_schematic_wires` takes its path with `get_path`; `expand_bus` (same
/// gateway, same absent-key situation) takes its `name` with `require_str`.
/// Both are read-only, so the gateway's mode gate lets them through.
#[tokio::test]
async fn an_absent_path_argument_is_an_invalid_argument_like_any_other() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_analysis"})).await;
    call(&handler, "load_toolset", json!({"name": "sch_buses"})).await;

    let from_get_path = invoke_entry(&handler, "list_schematic_wires", json!({})).await;
    let from_require_str = invoke_entry(&handler, "expand_bus", json!({})).await;

    assert_eq!(from_get_path["ok"], false, "{from_get_path}");
    assert_eq!(
        from_get_path["error_kind"], "invalid_argument",
        "an argument that was never supplied is not the tool failing: {from_get_path}"
    );
    assert_eq!(
        from_get_path["error_kind"], from_require_str["result"]["error"]["kind"],
        "one vocabulary, whichever helper the handler reached for: \
         {from_get_path} vs {from_require_str}"
    );
}

/// The guard against classifying too widely: a path that *was* supplied but
/// cannot be used is the tool trying and failing, and keeps the file error it
/// always had. Only "you never said" is an argument error.
#[tokio::test]
async fn a_path_that_is_present_but_unusable_is_still_the_tools_failure() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_analysis"})).await;

    let absent_file = dir.path().join("no-such.kicad_sch");
    let entry = invoke_entry(
        &handler,
        "list_schematic_wires",
        json!({ "schematic": absent_file.to_str().unwrap() }),
    )
    .await;

    assert_eq!(entry["ok"], false, "{entry}");
    assert_ne!(
        entry["error_kind"], "invalid_argument",
        "the caller supplied the argument; the file is what failed: {entry}"
    );
}

/// The same key, supplied, and the tool runs: the classification cannot be
/// reading anything but absence.
#[tokio::test]
async fn a_supplied_path_still_runs_the_tool() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();

    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_analysis"})).await;

    let entry = invoke_entry(
        &handler,
        "list_schematic_wires",
        json!({ "schematic": sch_path.to_str().unwrap() }),
    )
    .await;

    assert_eq!(entry["ok"], true, "a complete call is untouched: {entry}");
}
