//! D.8/D.8.3/INV4: a write tool called under `ReadOnly` is refused before
//! its handler ever runs, whether called directly (`tools/call`) or through
//! the `kicad_invoke` gateway. `Manufacturing` (the design freeze) refuses
//! only the subset of writes that could touch a source design document,
//! same two call sites — a `Derived` write (e.g. an `export_*`) passes.
//! `Write` (the default) sees no change in behaviour, `Experimental`
//! behaves exactly like `Write`, and an all-reads gateway batch is not
//! blocked wholesale by the envelope's own `Write` classification.
//!
//! Goes through [`McpHandler::handle_message`] end to end rather than
//! calling a handler directly, so the assertion is about the real dispatch
//! path (`mcp::handler::dispatch_tool`) and the real gateway
//! (`router::meta_tools::handle_kicad_invoke`), not about
//! `mode_gate::check` in isolation.

mod harness;

use konnect_core::mcp::handler::McpHandler;
use konnect_core::tools::ServerConfig;
use serde_json::{json, Value};
use std::path::Path;

async fn handler_with_mode(mode: kam_state::OperatingMode, project_dir: &Path) -> McpHandler {
    let config = ServerConfig {
        kicad_cli: String::new(),
        kicad_binary: String::new(),
        ipc_address: String::new(),
        project_dir: Some(project_dir.to_path_buf()),
        jlcpcb_db_path: None,
        auto_load_toolsets: false,
        mode,
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

/// The tool result's JSON body (mirrors `harness::body`, but the value here
/// is a full `JsonRpcResponse`, not a bare `CallToolResult`).
fn tool_body(resp: &Value) -> Value {
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("tool result carries text content");
    serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
}

fn is_error(resp: &Value) -> bool {
    resp["result"]["isError"].as_bool().unwrap_or(false)
}

#[tokio::test]
async fn read_only_refuses_a_direct_write_before_it_touches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::ReadOnly, dir.path()).await;
    // Discovery is unaffected by the mode: loading the toolset is a Read
    // meta-tool and must succeed even under ReadOnly.
    let loaded = call(&handler, "load_toolset", json!({"name": "sch_components"})).await;
    assert!(!is_error(&loaded), "load_toolset must not be mode-gated");

    let resp = call(
        &handler,
        "add_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "lib_id": "Device:R",
            "x": 120.0,
            "y": 60.0,
        }),
    )
    .await;

    assert!(is_error(&resp), "a write must be refused under ReadOnly");
    let body = tool_body(&resp);
    assert_eq!(body["error"]["kind"], "write_refused_by_mode");
    assert_eq!(body["error"]["tool"], "add_schematic_component");
    assert_eq!(body["error"]["transient"], "none");

    let after = std::fs::read(&sch_path).unwrap();
    assert_eq!(
        before, after,
        "the file must be byte-identical: nothing ran"
    );
}

#[tokio::test]
async fn write_mode_leaves_todays_behaviour_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Write, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "add_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "lib_id": "Device:R",
            "x": 120.0,
            "y": 60.0,
        }),
    )
    .await;

    assert!(!is_error(&resp), "Write must allow the same call: {resp}");
    let after = std::fs::read(&sch_path).unwrap();
    assert_ne!(before, after, "sanity: the call actually mutated the file");
}

#[tokio::test]
async fn read_only_refuses_a_gateway_batch_entry_before_it_touches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::ReadOnly, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "kicad_invoke",
        json!({
            "calls": [
                {
                    "tool": "add_schematic_component",
                    "args": {
                        "schematic": sch_path.to_str().unwrap(),
                        "lib_id": "Device:R",
                        "x": 120.0,
                        "y": 60.0,
                    }
                }
            ]
        }),
    )
    .await;

    // The envelope itself always succeeds (per-call detail lives inside).
    assert!(
        !is_error(&resp),
        "kicad_invoke's own call must not error: {resp}"
    );
    let body = tool_body(&resp);
    let entry = &body["results"][0];
    assert_eq!(entry["ok"], false);
    assert_eq!(entry["error_kind"], "write_refused_by_mode");

    let after = std::fs::read(&sch_path).unwrap();
    assert_eq!(
        before, after,
        "the gateway must refuse the entry before its handler runs"
    );
}

#[tokio::test]
async fn manufacturing_refuses_a_design_document_write_before_it_touches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Manufacturing, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "add_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "lib_id": "Device:R",
            "x": 120.0,
            "y": 60.0,
        }),
    )
    .await;

    assert!(
        is_error(&resp),
        "a design-document write must be refused under Manufacturing"
    );
    let body = tool_body(&resp);
    assert_eq!(body["error"]["kind"], "write_refused_by_mode");
    assert_eq!(body["error"]["tool"], "add_schematic_component");
    assert_eq!(body["error"]["transient"], "none");

    let after = std::fs::read(&sch_path).unwrap();
    assert_eq!(
        before, after,
        "the file must be byte-identical: nothing ran"
    );
}

#[tokio::test]
async fn manufacturing_allows_a_derived_write() {
    let dir = tempfile::tempdir().unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Manufacturing, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "task"})).await;

    // start_task (Domain::Task, Adapter::Internal): its write only touches
    // this server's own durable task state, never a project source document
    // — WriteTarget::Derived, chosen over an export_* tool so this test
    // needs no kicad-cli binary configured.
    let resp = call(
        &handler,
        "start_task",
        json!({ "objective": "verify Manufacturing allows a Derived write" }),
    )
    .await;

    assert!(
        !is_error(&resp),
        "a Derived write (start_task) must pass under Manufacturing: {resp}"
    );
}

#[tokio::test]
async fn write_mode_mutates_a_design_document() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Write, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "add_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "lib_id": "Device:R",
            "x": 120.0,
            "y": 60.0,
        }),
    )
    .await;

    assert!(!is_error(&resp), "Write must allow the same call: {resp}");
    let after = std::fs::read(&sch_path).unwrap();
    assert_ne!(before, after, "sanity: the call actually mutated the file");
}

#[tokio::test]
async fn experimental_behaves_exactly_like_write_including_the_manufacturing_refused_tool() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Experimental, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "add_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "lib_id": "Device:R",
            "x": 120.0,
            "y": 60.0,
        }),
    )
    .await;

    assert!(
        !is_error(&resp),
        "Experimental is a deliberate alias of Write: same call must succeed: {resp}"
    );
    let after = std::fs::read(&sch_path).unwrap();
    assert_ne!(before, after, "sanity: the call actually mutated the file");
}

#[tokio::test]
async fn manufacturing_refuses_a_gateway_batch_entry_before_it_touches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let before = std::fs::read(&sch_path).unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::Manufacturing, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "kicad_invoke",
        json!({
            "calls": [
                {
                    "tool": "add_schematic_component",
                    "args": {
                        "schematic": sch_path.to_str().unwrap(),
                        "lib_id": "Device:R",
                        "x": 120.0,
                        "y": 60.0,
                    }
                }
            ]
        }),
    )
    .await;

    assert!(
        !is_error(&resp),
        "kicad_invoke's own call must not error: {resp}"
    );
    let body = tool_body(&resp);
    let entry = &body["results"][0];
    assert_eq!(entry["ok"], false);
    assert_eq!(entry["error_kind"], "write_refused_by_mode");

    let after = std::fs::read(&sch_path).unwrap();
    assert_eq!(
        before, after,
        "the gateway must refuse the entry before its handler runs"
    );
}

#[tokio::test]
async fn read_only_does_not_block_an_all_reads_gateway_batch() {
    let dir = tempfile::tempdir().unwrap();
    let sch_path = dir.path().join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();

    let handler = handler_with_mode(kam_state::OperatingMode::ReadOnly, dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "kicad_invoke",
        json!({
            "calls": [
                {
                    "tool": "list_schematic_components",
                    "args": { "schematic": sch_path.to_str().unwrap() }
                }
            ]
        }),
    )
    .await;

    assert!(
        !is_error(&resp),
        "kicad_invoke's own call must not error: {resp}"
    );
    let body = tool_body(&resp);
    // The batch's own classification (Write, for the coverage audit) must
    // not stop a batch whose entries are all reads — only a per-entry
    // refusal may do that, and there is none here.
    assert_eq!(
        body["results"][0]["ok"], true,
        "an all-reads batch must run under ReadOnly: {body}"
    );
}
