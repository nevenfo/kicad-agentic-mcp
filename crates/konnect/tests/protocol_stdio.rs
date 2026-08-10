//! MCP protocol tests over stdio — spawn the real binary and speak JSON-RPC.
//!
//! Codifies the smoke tests that were run by hand at release time: handshake,
//! toolset loading for the entire registry, a real file-based tool call, and
//! the structured-error taxonomy the LLM relies on for recovery.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpProcess {
    fn spawn() -> Self {
        Self::spawn_in_dir(None)
    }

    /// Spawn with the process working directory set to `dir`, so
    /// `Config::load()`'s first search path (`konnect.toml` in cwd) picks up
    /// a test config file placed there.
    fn spawn_in_dir(dir: Option<&std::path::Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_konnect"));
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn konnect binary");
        let stdin = child.stdin.take().unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut p = McpProcess {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        // MCP handshake
        let init = p.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "protocol-test", "version": "0"}
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "konnect");
        p.notify("notifications/initialized");
        p
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{}", msg).unwrap();
        self.stdin.flush().unwrap();
        // Read lines until the response with our id arrives (skips any
        // notifications the server might emit).
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).unwrap();
            assert!(
                n > 0,
                "server closed stdout waiting for response to {method}"
            );
            let v: Value = serde_json::from_str(line.trim()).unwrap();
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return v;
            }
        }
    }

    fn notify(&mut self, method: &str) {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        writeln!(self.stdin, "{}", msg).unwrap();
        self.stdin.flush().unwrap();
    }

    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({"name": name, "arguments": args}));
        resp["result"].clone()
    }

    /// Send a `tools/call`, then a fencing `ping`, and return every line the
    /// server emits up to and including the ping response. The fence
    /// guarantees the read loop terminates even when the tool call emits no
    /// notification (as in bug #19), so a test can assert on side-effect
    /// notifications without risking a hang.
    fn call_tool_then_fence(&mut self, name: &str, args: Value) -> Vec<Value> {
        let call_id = self.next_id;
        self.next_id += 1;
        let call = json!({
            "jsonrpc": "2.0", "id": call_id, "method": "tools/call",
            "params": {"name": name, "arguments": args}
        });
        writeln!(self.stdin, "{}", call).unwrap();
        let fence_id = self.next_id;
        self.next_id += 1;
        let fence = json!({"jsonrpc": "2.0", "id": fence_id, "method": "ping", "params": {}});
        writeln!(self.stdin, "{}", fence).unwrap();
        self.stdin.flush().unwrap();

        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).unwrap();
            assert!(n > 0, "server closed stdout before fence response");
            let v: Value = serde_json::from_str(line.trim()).unwrap();
            let is_fence = v.get("id").and_then(Value::as_i64) == Some(fence_id);
            lines.push(v);
            if is_fence {
                break;
            }
        }
        lines
    }

    /// Parse the JSON body of a tool result's first text content.
    fn tool_body(result: &Value) -> Value {
        let text = result["content"][0]["text"].as_str().unwrap_or("{}");
        serde_json::from_str(text).unwrap_or(Value::Null)
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn handshake_baseline_and_full_registry_loads() {
    let mut p = McpProcess::spawn();

    // Baseline tools/list: starter kit + meta-tools only (small context).
    let list = p.request("tools/list", json!({}));
    let baseline = list["result"]["tools"].as_array().unwrap().len();
    assert!(
        (10..30).contains(&baseline),
        "baseline tools/list should be the small starter kit, got {baseline}"
    );

    // list_toolboxes reports the registry; every toolset must load.
    let boxes = McpProcess::tool_body(&p.call_tool("list_toolboxes", json!({})));
    let toolsets: Vec<String> = boxes["toolsets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        toolsets.len() >= 17,
        "expected 17+ toolsets, got {}",
        toolsets.len()
    );
    // No license-era fields may reappear.
    assert!(boxes.get("license_tier").is_none());
    assert!(boxes["toolsets"][0].get("tier").is_none());

    let mut total = 0u64;
    for name in &toolsets {
        let loaded = McpProcess::tool_body(&p.call_tool("load_toolset", json!({"name": name})));
        let added = loaded["tools_added"].as_u64().unwrap_or(0);
        assert!(added > 0, "toolset '{name}' loaded no tools");
        total += added;
    }
    assert_eq!(
        total,
        boxes["total_tools"].as_u64().unwrap(),
        "sum of loaded tools disagrees with list_toolboxes total"
    );
}

#[test]
fn file_based_tool_roundtrip_in_temp_project() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proto_demo");
    let mut p = McpProcess::spawn();

    let created = p.call_tool(
        "create_project",
        json!({"name": "proto_demo", "path": proj.to_string_lossy()}),
    );
    assert_ne!(
        created["isError"],
        json!(true),
        "create_project failed: {created}"
    );
    assert!(proj.join("proto_demo.kicad_sch").exists());

    let info = p.call_tool(
        "get_project_info",
        json!({"path": proj.join("proto_demo.kicad_pro").to_string_lossy()}),
    );
    assert_ne!(
        info["isError"],
        json!(true),
        "get_project_info failed: {info}"
    );
}

#[test]
fn structured_errors_guide_recovery() {
    let mut p = McpProcess::spawn();

    // Known tool in an unloaded toolset → toolset_not_loaded naming the owner.
    let r = p.call_tool("route_trace", json!({}));
    assert_eq!(r["isError"], json!(true));
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["error"]["kind"], "toolset_not_loaded");
    assert_eq!(body["error"]["toolset"], "pcb_routing");

    // Unknown tool → unknown_tool.
    let r = p.call_tool("frobnicate_board", json!({}));
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["error"]["kind"], "unknown_tool");

    // Missing required argument → invalid_argument naming the field.
    let r = p.call_tool("create_project", json!({"path": "/tmp/x"}));
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["error"]["kind"], "invalid_argument");
    assert_eq!(body["error"]["field"], "name");
}

#[test]
fn unknown_method_is_json_rpc_error_not_crash() {
    let mut p = McpProcess::spawn();
    let resp = p.request("tools/definitely_not_a_method", json!({}));
    assert!(
        resp.get("error").is_some(),
        "expected JSON-RPC error: {resp}"
    );
    // Server must still be alive afterwards.
    let ping = p.request("ping", json!({}));
    assert!(ping.get("result").is_some());
}

/// Regression test for issue #19. After `load_toolset`, the server must emit
/// `notifications/tools/list_changed` **over stdio** — not only over HTTP/SSE.
/// Without it, stdio clients (Claude Code) never re-fetch `tools/list`, so
/// every tool added by `load_toolset` stays uncallable for the session.
#[test]
fn load_toolset_emits_list_changed_over_stdio() {
    let mut p = McpProcess::spawn();
    let lines = p.call_tool_then_fence("load_toolset", json!({"name": "sch_components"}));
    let saw_notification = lines.iter().any(|v| {
        v.get("method").and_then(Value::as_str) == Some("notifications/tools/list_changed")
            && v.get("id").is_none()
    });
    assert!(
        saw_notification,
        "expected notifications/tools/list_changed after load_toolset (issue #19); saw: {lines:#?}"
    );
}

/// The same guarantee for `unload_toolset` — removing tools must also tell the
/// client to refresh its tool list.
#[test]
fn unload_toolset_emits_list_changed_over_stdio() {
    let mut p = McpProcess::spawn();
    let _ = p.call_tool_then_fence("load_toolset", json!({"name": "sch_components"}));
    let lines = p.call_tool_then_fence("unload_toolset", json!({"name": "sch_components"}));
    let saw_notification = lines.iter().any(|v| {
        v.get("method").and_then(Value::as_str) == Some("notifications/tools/list_changed")
            && v.get("id").is_none()
    });
    assert!(
        saw_notification,
        "expected notifications/tools/list_changed after unload_toolset; saw: {lines:#?}"
    );
}

/// `load_toolset` accepts an array of names in one call: all listed toolsets
/// load, tools_added sums across them, and only one list_changed notification
/// fires for the whole batch.
#[test]
fn load_toolset_batch_form_loads_all_and_notifies_once() {
    let mut p = McpProcess::spawn();
    let lines = p.call_tool_then_fence(
        "load_toolset",
        json!({"name": ["sch_components", "sch_wiring"]}),
    );
    let r = lines
        .iter()
        .find(|v| v.get("result").is_some())
        .expect("expected a tools/call result")["result"]
        .clone();
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["tools_added"].as_u64(), Some(36));
    // `tools` items are bare name strings. Echoing each description here used
    // to be the single largest response-token line item in the whole protocol
    // (2 208 tokens per task on the golden suite, 18% of everything the harness
    // received) and it was pure duplication: this call emits
    // notifications/tools/list_changed, the client re-fetches tools/list, and
    // the very same descriptions arrive again with their schemas attached.
    let tools = body["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 36);
    for t in tools {
        assert!(
            t.as_str().is_some(),
            "tools must be bare name strings, not objects: {t:#?}"
        );
    }
    assert!(tools
        .iter()
        .any(|t| t.as_str() == Some("add_schematic_component")));

    let notification_count = lines
        .iter()
        .filter(|v| {
            v.get("method").and_then(Value::as_str) == Some("notifications/tools/list_changed")
                && v.get("id").is_none()
        })
        .count();
    assert_eq!(
        notification_count, 1,
        "expected exactly one list_changed notification for the batch; saw: {lines:#?}"
    );

    // Mixed valid/invalid names: partial failure is not isError, but the
    // errors array names the unknown toolset and loaded lists only the real one.
    let lines = p.call_tool_then_fence(
        "load_toolset",
        json!({"name": ["templates", "bogus_toolset"]}),
    );
    let r = lines
        .iter()
        .find(|v| v.get("result").is_some())
        .expect("expected a tools/call result")["result"]
        .clone();
    assert_ne!(r["isError"].as_bool(), Some(true), "{r:#?}");
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["loaded"], json!(["templates"]));
    let errors = body["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].as_str().unwrap().contains("list_toolboxes"),
        "{errors:#?}"
    );
}

/// All names in one `load_toolset` call unknown -> a typed `invalid_argument`
/// error (not a JSON body with a hand-set `isError`), so the observer keeps a
/// real `error_kind` column instead of degrading to `handler_error`.
#[test]
fn load_toolset_batch_total_failure_is_typed_error() {
    let mut p = McpProcess::spawn();
    let r = p.call_tool("load_toolset", json!({"name": ["bogus_one", "bogus_two"]}));
    assert_eq!(r["isError"], json!(true));
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["error"]["kind"], "invalid_argument");
    assert_eq!(body["error"]["field"], "name");
    assert!(
        body["message"].as_str().unwrap().contains("list_toolboxes"),
        "{body:#?}"
    );
}

/// With `auto_load_toolsets = true` in `konnect.toml` (picked up from the
/// server process's cwd), calling a tool from an unloaded toolset auto-loads
/// it and executes in the same call instead of returning `toolset_not_loaded`.
/// Default-off behavior (no config file) is covered by
/// `structured_errors_guide_recovery`.
#[test]
fn auto_load_toolsets_config_loads_and_executes_on_miss() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("konnect.toml"),
        "auto_load_toolsets = true\n",
    )
    .unwrap();
    let mut p = McpProcess::spawn_in_dir(Some(tmp.path()));

    // route_trace is in pcb_routing, not loaded at startup. With auto-load on,
    // the toolset loads, a list_changed notification fires, and the call
    // reaches the handler's own missing-argument check (net_name) instead of
    // failing with toolset_not_loaded.
    let lines = p.call_tool_then_fence("route_trace", json!({}));
    let r = lines
        .iter()
        .find(|v| v.get("result").is_some())
        .expect("expected a tools/call result")["result"]
        .clone();
    assert_eq!(r["isError"], json!(true));
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["error"]["kind"], "invalid_argument");
    assert_eq!(body["error"]["field"], "net_name");

    let saw_notification = lines.iter().any(|v| {
        v.get("method").and_then(Value::as_str) == Some("notifications/tools/list_changed")
            && v.get("id").is_none()
    });
    assert!(
        saw_notification,
        "expected notifications/tools/list_changed after auto-load; saw: {lines:#?}"
    );
}

/// The gateway's whole reason to exist: `kicad_invoke` runs a tool that is not
/// in `tools/list` and the catalogue does not change, so no
/// `notifications/tools/list_changed` fires and the client never re-fetches.
/// On the golden suite that refresh is 2 281 tokens per task — more than twice
/// the tool output it accompanies.
#[test]
fn kicad_invoke_runs_an_unloaded_tool_without_touching_the_catalogue() {
    let mut p = McpProcess::spawn();

    // Baseline catalogue, captured before the call.
    let before: Vec<String> = p.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !before.contains(&"get_symbol_info".to_string()),
        "get_symbol_info must not be loaded at startup for this test to mean anything"
    );

    let lines = p.call_tool_then_fence(
        "kicad_invoke",
        json!({"calls": [{"tool": "get_symbol_info", "args": {}}]}),
    );
    let saw_notification = lines.iter().any(|v| {
        v.get("method").and_then(Value::as_str) == Some("notifications/tools/list_changed")
            && v.get("id").is_none()
    });
    assert!(
        !saw_notification,
        "kicad_invoke must not change the catalogue; saw: {lines:#?}"
    );

    // The call reached the real handler — it failed on its own argument check,
    // not on toolset_not_loaded.
    let r = lines
        .iter()
        .find(|v| v.get("result").is_some())
        .expect("expected a tools/call result")["result"]
        .clone();
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["count"].as_u64(), Some(1));
    assert_eq!(body["ok"].as_u64(), Some(0));
    assert_eq!(body["results"][0]["ok"], json!(false));
    assert_eq!(
        body["results"][0]["result"]["error"]["kind"], "invalid_argument",
        "expected the tool's own validation error, got {body:#?}"
    );

    let after: Vec<String> = p.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(before, after, "the catalogue must be byte-identical");
}

/// A batch runs in order, and by default stops at the first failure rather than
/// applying the rest of a plan on top of a state that already went wrong. The
/// calls that did not run are reported so the caller knows what to retry.
#[test]
fn kicad_invoke_batches_in_order_and_stops_on_error() {
    let dir = std::env::temp_dir().join(format!("konnect-gw-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    let mut p = McpProcess::spawn();
    let r = p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": dir.to_str().unwrap(), "name": "gw"}},
            {"tool": "get_symbol_info", "args": {}},
            {"tool": "list_schematic_components", "args": {"schematic": dir.join("gw.kicad_sch")}}
        ]}),
    );
    let body = McpProcess::tool_body(&r);
    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert_eq!(body["failed_at"].as_u64(), Some(1));
    assert_eq!(body["not_run"].as_u64(), Some(1));
    assert_eq!(body["results"][0]["tool"], "create_project");
    assert_eq!(body["results"][0]["ok"], json!(true));

    // Every inner call is auditable under its own name, not swallowed by the
    // batch: a mutation without an audit record is the one thing batching must
    // not buy.
    let recent = McpProcess::tool_body(&p.call_tool("get_recent_calls", json!({"limit": 20})));
    let names: Vec<&str> = recent["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["tool"].as_str())
        .collect();
    assert!(
        names.contains(&"create_project") && names.contains(&"get_symbol_info"),
        "inner calls missing from the call log: {names:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `kicad_describe` hands back the schema of a tool that is not loaded, and
/// names what it could not find instead of failing the whole call.
#[test]
fn kicad_describe_returns_schemas_without_loading_anything() {
    let mut p = McpProcess::spawn();
    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_describe",
        json!({"names": ["connect_pins", "no_such_tool"]}),
    ));
    assert_eq!(body["count"].as_u64(), Some(1));
    assert_eq!(body["tools"][0]["name"], "connect_pins");
    assert!(
        body["tools"][0]["input_schema"]["properties"].is_object(),
        "expected a real input schema, got {body:#?}"
    );
    assert_eq!(body["not_found"], json!(["no_such_tool"]));

    let listed: Vec<String> = p.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !listed.contains(&"connect_pins".to_string()),
        "kicad_describe must not admit the tool into tools/list"
    );
}

// ─── Phase D: the gateway as a transaction ───────────────────────────────────

/// A scratch project directory unique to one test, cleaned up on drop.
struct Scratch {
    dir: std::path::PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "konnect-txn-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn path(&self) -> &str {
        self.dir.to_str().unwrap()
    }

    fn sch(&self, name: &str) -> std::path::PathBuf {
        self.dir.join(format!("{name}.kicad_sch"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A batch that fails halfway leaves nothing behind. Before Phase D the label
/// added by the first call survived the failure of the second, so the project
/// ended in a state the caller neither asked for nor was told about.
#[test]
fn kicad_invoke_rolls_back_a_batch_that_fails_halfway() {
    let scratch = Scratch::new("rollback");
    let mut p = McpProcess::spawn();

    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "rb"}}
        ]}),
    ));
    assert_eq!(created["ok"].as_u64(), Some(1), "{created:#?}");
    assert!(
        created["revisions"].is_object(),
        "a mutating batch must report the revision of what it wrote: {created:#?}"
    );

    let sch = scratch.sch("rb");
    let before = std::fs::read_to_string(&sch).unwrap();

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "add_schematic_net_label",
             "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}},
            {"tool": "get_symbol_info", "args": {}}
        ]}),
    ));

    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert_eq!(body["failed_at"].as_u64(), Some(1));
    assert_eq!(
        body["rolled_back"]["restored"].as_u64(),
        Some(1),
        "the successful first call must have been undone: {body:#?}"
    );
    assert_eq!(
        std::fs::read_to_string(&sch).unwrap(),
        before,
        "the schematic must be byte-identical to before the batch"
    );
    assert!(
        body["revisions"].is_null(),
        "a rolled-back batch produced no new revision: {body:#?}"
    );
}

/// `atomic: false` is the opt-out for a caller that wants whatever succeeded.
#[test]
fn kicad_invoke_keeps_partial_work_when_atomic_is_off() {
    let scratch = Scratch::new("nonatomic");
    let mut p = McpProcess::spawn();
    p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "na"}}
        ]}),
    );

    let sch = scratch.sch("na");
    let before = std::fs::read_to_string(&sch).unwrap();

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"atomic": false, "calls": [
            {"tool": "add_schematic_net_label",
             "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}},
            {"tool": "get_symbol_info", "args": {}}
        ]}),
    ));

    assert_eq!(body["failed_at"].as_u64(), Some(1), "{body:#?}");
    assert!(body["rolled_back"].is_null());
    assert_ne!(
        std::fs::read_to_string(&sch).unwrap(),
        before,
        "atomic: false means the caller keeps what succeeded"
    );
}

/// `done=true` is not a reviewable answer. A batch says what it changed in the
/// vocabulary of the design — and by default says it in one line, so the audit
/// trail costs a sentence rather than a transcript.
#[test]
fn kicad_invoke_reports_what_it_changed_in_design_terms() {
    let scratch = Scratch::new("semdiff");
    let mut p = McpProcess::spawn();
    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "sd"}}
        ]}),
    ));
    assert_eq!(
        created["diff"]["summary"].as_str(),
        Some("document +3"),
        "creating a project is a change, not the absence of one: {created:#?}"
    );
    let sch = scratch.sch("sd");

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "batch_place_components", "args": {"schematic": sch, "components": [
                {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25}
            ]}}
        ]}),
    ));

    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert_eq!(
        body["diff"]["summary"].as_str(),
        Some("symbol +2"),
        "the reply must name the design change, not the file count: {body:#?}"
    );
    assert!(
        body["diff"]["changes"].is_null(),
        "the per-item detail is opt-in — it is not in the default reply: {body:#?}"
    );
}

/// The detail is one argument away, and `none` turns the whole thing off for a
/// caller that is paying for every token.
#[test]
fn the_diff_detail_level_is_the_callers_choice() {
    let scratch = Scratch::new("semdifflevel");
    let mut p = McpProcess::spawn();
    p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "dl"}}
        ]}),
    );
    let sch = scratch.sch("dl");

    let detailed = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"diff": "changes", "calls": [
            {"tool": "add_schematic_net_label",
             "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}
        ]}),
    ));
    let changes = detailed["diff"]["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("expected per-item changes: {detailed:#?}"));
    assert!(
        changes
            .iter()
            .any(|c| c.as_str() == Some("label VOUT added")),
        "a placed label must be named: {changes:#?}"
    );

    let silent = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"diff": "none", "calls": [
            {"tool": "add_schematic_net_label",
             "args": {"schematic": sch, "net": "VIN", "x": 100.33, "y": 74.93}}
        ]}),
    ));
    assert!(
        silent["diff"].is_null(),
        "diff: none must cost nothing: {silent:#?}"
    );
    assert!(
        silent["revisions"].is_object(),
        "turning the diff off must not turn off revision reporting: {silent:#?}"
    );
}

/// A plan compiled against a document the user has since edited is refused,
/// not applied on top. This is the half of the pair a file-level rollback
/// cannot provide: detection.
#[test]
fn kicad_invoke_refuses_a_stale_base_revision_without_applying_anything() {
    let scratch = Scratch::new("stale");
    let mut p = McpProcess::spawn();
    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "st"}}
        ]}),
    ));

    let sch = scratch.sch("st");
    let root = created["revisions_root"].as_str().map(String::from);
    let key = match &root {
        Some(_) => "st.kicad_sch".to_string(),
        None => sch.to_string_lossy().into_owned(),
    };
    let revision = created["revisions"][&key]
        .as_str()
        .unwrap_or_else(|| panic!("no revision for {key}: {created:#?}"))
        .to_string();

    // The batch built against that revision applies.
    let mut ok_args = json!({
        "base_revisions": {key.clone(): revision.clone()},
        "calls": [{"tool": "add_schematic_net_label",
                   "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}]
    });
    if let Some(root) = &root {
        ok_args["base_revisions_root"] = json!(root);
    }
    let applied = McpProcess::tool_body(&p.call_tool("kicad_invoke", ok_args.clone()));
    assert_eq!(applied["ok"].as_u64(), Some(1), "{applied:#?}");

    // The same batch replayed against the now-old revision is refused.
    let after = std::fs::read_to_string(&sch).unwrap();
    let stale = p.call_tool("kicad_invoke", ok_args);
    let body = McpProcess::tool_body(&stale);
    assert_eq!(stale["isError"], json!(true), "{body:#?}");
    assert_eq!(body["error"]["kind"], "stale_revision");
    assert_eq!(body["error"]["transient"], "state");
    assert_eq!(body["error"]["expected"], revision);
    assert_eq!(
        std::fs::read_to_string(&sch).unwrap(),
        after,
        "a refused batch must not have run a single call"
    );
}

/// A client that times out and retries must not get its parts added twice.
#[test]
fn kicad_invoke_replays_an_operation_id_instead_of_applying_it_twice() {
    let scratch = Scratch::new("idem");
    let mut p = McpProcess::spawn();
    p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "id"}}
        ]}),
    );

    let sch = scratch.sch("id");
    let batch = json!({
        "operation_id": "op_test_1",
        "calls": [{"tool": "add_schematic_net_label",
                   "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}]
    });

    let first = McpProcess::tool_body(&p.call_tool("kicad_invoke", batch.clone()));
    assert_eq!(first["ok"].as_u64(), Some(1), "{first:#?}");
    let after_first = std::fs::read_to_string(&sch).unwrap();

    let second = McpProcess::tool_body(&p.call_tool("kicad_invoke", batch));
    assert_eq!(second["replayed"], json!(true), "{second:#?}");
    assert_eq!(second["ok"].as_u64(), Some(1));
    assert_eq!(
        std::fs::read_to_string(&sch).unwrap(),
        after_first,
        "the replay must not have added a second label"
    );
}

/// `stop_on_error: false` says the calls are independent and the survivors are
/// wanted. Rolling them back would be the opposite of the request, so atomic
/// follows stop_on_error unless it is set explicitly. Found by the benchmark:
/// the recovery task deliberately fails five calls mid-batch and still expects
/// the design built by the rest, and an unconditional rollback scored it 0/3.
#[test]
fn a_continue_on_error_batch_is_not_rolled_back_by_default() {
    let scratch = Scratch::new("continue");
    let mut p = McpProcess::spawn();
    p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "co"}}
        ]}),
    );

    let sch = scratch.sch("co");
    let before = std::fs::read_to_string(&sch).unwrap();

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"stop_on_error": false, "calls": [
            {"tool": "get_symbol_info", "args": {}},
            {"tool": "add_schematic_net_label",
             "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}
        ]}),
    ));

    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert!(body["rolled_back"].is_null(), "{body:#?}");
    assert_ne!(
        std::fs::read_to_string(&sch).unwrap(),
        before,
        "the call that succeeded after the failure must survive"
    );
}
