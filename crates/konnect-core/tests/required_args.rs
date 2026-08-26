//! P.6.9.10: the `required` list a tool advertises is enforced at dispatch.
//!
//! Every tool declares `"required"` in its `input_schema`, and until now
//! nothing read it: an argument a schema calls mandatory reached the handler,
//! which refused it (or not) on its own terms, at whatever point in its body
//! it happened to look. This checks the floor instead — a missing required
//! key is refused before the handler runs, in the same `invalid_argument`
//! shape the `require_*` helpers emit, so a client sees one vocabulary.
//!
//! Goes through [`McpHandler::handle_message`] end to end, like
//! `mode_gate.rs`, so the assertion is about the real dispatch path
//! (`mcp::handler::dispatch_tool`), not a helper in isolation.

mod harness;

use konnect_core::mcp::handler::McpHandler;
use konnect_core::tools::ServerConfig;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

async fn handler_at(project_dir: &Path) -> McpHandler {
    handler_in_mode(project_dir, kam_state::OperatingMode::Write).await
}

async fn handler_in_mode(project_dir: &Path, mode: kam_state::OperatingMode) -> McpHandler {
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

/// `tools/call` with no `arguments` member at all — the shape
/// `execute_tool` turns into `{}`.
async fn call_without_arguments(handler: &McpHandler, name: &str) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name }
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

fn is_error(resp: &Value) -> bool {
    resp["result"]["isError"].as_bool().unwrap_or(false)
}

/// A well-formed UUID that addresses no symbol in any fixture.
const UNKNOWN_UUID: &str = "00000000-0000-4000-8000-00000000dead";

/// A copy of the two-resistor fixture in a scratch dir, plus its bytes, so a
/// test can prove the handler never got as far as touching it.
fn fixture_copy(dir: &Path) -> (PathBuf, Vec<u8>) {
    let sch_path = dir.join("board.kicad_sch");
    std::fs::copy(
        harness::fixtures_dir().join(harness::TWO_RESISTORS),
        &sch_path,
    )
    .unwrap();
    let bytes = std::fs::read(&sch_path).unwrap();
    (sch_path, bytes)
}

/// `move_schematic_component` declares `required: ["schematic", "x", "y"]`
/// and its handler resolves the target symbol — reading the file off disk —
/// *before* it looks at `x`/`y`. That ordering is what makes "the handler
/// ran" observable: addressed by a `uuid` that is in no document, the
/// answer the handler produces is the resolver's `not_found`, which can only
/// be reached by opening the file. The dispatch check must get there first
/// and name the key the schema says is missing.
#[tokio::test]
async fn a_missing_required_key_is_refused_before_the_handler_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, before) = fixture_copy(dir.path());

    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "move_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "uuid": UNKNOWN_UUID,
            "x": 100.0,
        }),
    )
    .await;

    assert!(is_error(&resp), "a missing required key must be refused");
    let body = tool_body(&resp);
    assert_eq!(
        body["error"]["kind"], "invalid_argument",
        "schema-level refusal speaks the same kind as require_*: {body}"
    );
    assert_eq!(
        body["error"]["field"], "y",
        "the refusal names the missing key, not the resolver's complaint: {body}"
    );

    assert_eq!(
        before,
        std::fs::read(&sch_path).unwrap(),
        "nothing may have run"
    );
}

/// A tool called with no `arguments` at all is the same case: `{}` is missing
/// every required key, and the first one is named.
#[tokio::test]
async fn a_call_with_no_arguments_at_all_names_the_first_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call_without_arguments(&handler, "move_schematic_component").await;

    assert!(
        is_error(&resp),
        "no arguments cannot satisfy a required list"
    );
    let body = tool_body(&resp);
    assert_eq!(body["error"]["kind"], "invalid_argument", "{body}");
    assert_eq!(body["error"]["field"], "schematic", "{body}");
}

/// An explicit `null` is not a value: it is refused exactly like an absent
/// key, so a client that serialises its optionals as `null` gets the same
/// answer as one that omits them.
#[tokio::test]
async fn an_explicit_null_counts_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, before) = fixture_copy(dir.path());

    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "move_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "uuid": UNKNOWN_UUID,
            "x": 100.0,
            "y": null,
        }),
    )
    .await;

    assert!(is_error(&resp), "an explicit null is not a value");
    let body = tool_body(&resp);
    assert_eq!(body["error"]["kind"], "invalid_argument", "{body}");
    assert_eq!(body["error"]["field"], "y", "{body}");
    assert_eq!(
        before,
        std::fs::read(&sch_path).unwrap(),
        "nothing may have run"
    );
}

/// The refusal is indistinguishable from the one a `require_*` helper emits
/// for the same key: same kind, same field. Only the reason differs, and a
/// client branches on the first two.
#[tokio::test]
async fn the_refusal_shape_matches_the_require_helpers() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, _) = fixture_copy(dir.path());

    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    // Present but wrong type: the key satisfies the schema's `required`, so
    // this is `require_f64` inside the handler answering.
    let from_helper = tool_body(
        &call(
            &handler,
            "move_schematic_component",
            json!({
                "schematic": sch_path.to_str().unwrap(),
                "reference": "R1",
                "x": 100.0,
                "y": "not a number",
            }),
        )
        .await,
    );
    // Absent: the dispatch check answering, ahead of a resolver that would
    // otherwise refuse this address first.
    let from_dispatch = tool_body(
        &call(
            &handler,
            "move_schematic_component",
            json!({
                "schematic": sch_path.to_str().unwrap(),
                "uuid": UNKNOWN_UUID,
                "x": 100.0,
            }),
        )
        .await,
    );

    assert_eq!(
        from_helper["error"]["kind"], from_dispatch["error"]["kind"],
        "one vocabulary: {from_helper} vs {from_dispatch}"
    );
    assert_eq!(
        from_helper["error"]["field"], from_dispatch["error"]["field"],
        "both name the same key: {from_helper} vs {from_dispatch}"
    );
    assert_eq!(from_dispatch["error"]["kind"], "invalid_argument");
}

/// A schema with an empty `required` list is unchanged: the check has nothing
/// to enforce and the call runs on no arguments at all.
#[tokio::test]
async fn a_tool_with_no_required_keys_still_takes_an_empty_call() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler_at(dir.path()).await;

    let resp = call(&handler, "get_recent_calls", json!({})).await;
    assert!(!is_error(&resp), "required: [] enforces nothing: {resp}");

    let resp = call_without_arguments(&handler, "server_stats").await;
    assert!(
        !is_error(&resp),
        "nor does a missing arguments member: {resp}"
    );
}

/// A complete call is untouched by the check — it still applies.
#[tokio::test]
async fn a_complete_call_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, before) = fixture_copy(dir.path());

    let handler = handler_at(dir.path()).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "move_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "reference": "R1",
            "x": 100.0,
            "y": 100.0,
        }),
    )
    .await;

    assert!(!is_error(&resp), "a complete call must still run: {resp}");
    assert_ne!(
        before,
        std::fs::read(&sch_path).unwrap(),
        "sanity: the call actually mutated the file"
    );
}

/// The meta-tool gateway is checked on its own envelope only. `calls` is
/// required and its absence is refused here; the per-entry `tool` key stays
/// the gateway's own business, entry by entry, as does mode gating.
#[tokio::test]
async fn the_gateway_envelope_is_checked_but_not_its_entries() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, _) = fixture_copy(dir.path());
    let handler = handler_at(dir.path()).await;

    let resp = call(&handler, "kicad_invoke", json!({})).await;
    assert!(is_error(&resp), "the envelope's own required list applies");
    let body = tool_body(&resp);
    assert_eq!(body["error"]["kind"], "invalid_argument", "{body}");
    assert_eq!(body["error"]["field"], "calls", "{body}");

    // An entry missing a key its own schema requires is still the gateway's
    // answer, per entry, with the envelope itself succeeding.
    let resp = call(
        &handler,
        "kicad_invoke",
        json!({
            "calls": [
                {
                    "tool": "move_schematic_component",
                    "args": {
                        "schematic": sch_path.to_str().unwrap(),
                        "reference": "R1",
                        "x": 100.0,
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
    assert_eq!(body["results"][0]["ok"], false, "{body}");
    assert_eq!(
        body["results"][0]["result"]["error"]["kind"], "invalid_argument",
        "{body}"
    );
}

/// Order against the mode gate: the gate wins.
///
/// A `ReadOnly` process refuses a write whatever its arguments say, and the
/// refusal a caller gets must be the one that is true of the call — not
/// coaching on how to complete an argument list for something that would be
/// refused anyway. The argument check therefore sits *after* the gate, on
/// both dispatch paths, and this is what pins that order: nothing else in the
/// suite calls a mode-gated tool with an incomplete argument list.
#[tokio::test]
async fn the_mode_gate_still_answers_first() {
    let dir = tempfile::tempdir().unwrap();
    let (sch_path, _) = fixture_copy(dir.path());

    let handler = handler_in_mode(dir.path(), kam_state::OperatingMode::ReadOnly).await;
    call(&handler, "load_toolset", json!({"name": "sch_components"})).await;

    let resp = call(
        &handler,
        "move_schematic_component",
        json!({
            "schematic": sch_path.to_str().unwrap(),
            "uuid": UNKNOWN_UUID,
            "x": 100.0,
        }),
    )
    .await;

    assert!(is_error(&resp));
    let body = tool_body(&resp);
    assert_eq!(
        body["error"]["kind"], "write_refused_by_mode",
        "the mode is the reason this call cannot run: {body}"
    );
}

/// P.6.9.17: a batch entry that fails through the handler's `Err(anyhow)` path
/// names the argument it could not do without, the same as one that fails
/// through `Ok(CallToolResult::error_kind)`.
///
/// The two paths assemble their result differently — the `Ok` half carries the
/// handler's whole structured body under `result`, `field` and all, while the
/// `Err` half is flattened at the gateway into `error_kind` plus a message.
/// `ToolErrorKind::from_anyhow` classifies a `MissingArgument` into
/// `InvalidArgument { field, .. }` (P.6.9.11 carried the field that far on
/// purpose), and the flattening then dropped it: the caller was told an
/// argument was invalid without being told which one. Which path a given
/// refusal takes is an implementation detail of the handler — `get_path`
/// returns `anyhow::Result`, `require_str` returns a `CallToolResult` — so a
/// batch caller cannot know in advance whether it will be told the field.
///
/// Matters most here of all places: batch entries skip the dispatch's
/// `required` check by design (D131), so this refusal is the *only* one they
/// get.
#[tokio::test]
async fn a_batch_entry_names_the_missing_field_on_either_failure_path() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler_at(dir.path()).await;

    let resp = call(
        &handler,
        "kicad_invoke",
        json!({
            "calls": [
                // `audit_decoupling` reads its path with `get_path`, so a
                // missing `schematic` leaves the handler as `Err(anyhow)`.
                { "tool": "audit_decoupling", "args": {} }
            ],
            "stop_on_error": false
        }),
    )
    .await;
    assert!(
        !is_error(&resp),
        "kicad_invoke's own call must not error: {resp}"
    );
    let body = tool_body(&resp);
    let entry = &body["results"][0];
    assert_eq!(entry["ok"], false, "{body}");
    assert_eq!(entry["error_kind"], "invalid_argument", "{body}");
    assert_eq!(
        entry["error_field"], "schematic",
        "an entry told its argument is invalid must be told which one: {body}"
    );

    // The one refusal the gateway assembles by hand, rather than classifying
    // from a handler's error: an entry with no `tool` key. It cannot even name
    // the tool it is about, so naming the key matters more here, not less.
    let resp = call(
        &handler,
        "kicad_invoke",
        json!({ "calls": [ { "args": {} } ], "stop_on_error": false }),
    )
    .await;
    let body = tool_body(&resp);
    let entry = &body["results"][0];
    assert_eq!(entry["error_kind"], "invalid_argument", "{body}");
    assert_eq!(entry["error_field"], "tool", "{body}");
}
