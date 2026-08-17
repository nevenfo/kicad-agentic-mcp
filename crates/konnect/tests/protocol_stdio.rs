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
        Self::spawn_with(dir, None)
    }

    /// Spawn with `KICAD10_SYMBOL_DIR` pointed at a fixture library, for a
    /// test that must actually place a symbol.
    ///
    /// The env var goes to the child, never to this process: these tests run
    /// in parallel threads of one binary, and a `set_var` here would race
    /// every other test in the file. It also makes the test say which
    /// libraries it needs instead of inheriting whatever KiCad the machine
    /// has — which is how `kicad_invoke_reports_what_it_changed_in_design_terms`
    /// passed on a developer box and failed on the first CI runner to ever
    /// run it (L.1.5).
    fn spawn_with_symbols(symbols: &std::path::Path) -> Self {
        Self::spawn_with(None, Some(symbols))
    }

    fn spawn_with(dir: Option<&std::path::Path>, symbols: Option<&std::path::Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_konnect"));
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        if let Some(symbols) = symbols {
            command.env("KICAD10_SYMBOL_DIR", symbols);
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
        // Evidence handles are only followable by a client that was told the
        // server serves resources, so the capability is part of the handshake
        // contract rather than an implementation detail.
        assert!(
            init["result"]["capabilities"]["resources"].is_object(),
            "the server must advertise resources: {init:#?}"
        );
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

/// The `Datasheet` value of the fixture symbol below, and nothing a real KiCAD
/// library would ever carry. A test that places `Device:R` can assert on it to
/// prove it resolved through the fixture rather than through whatever KiCAD the
/// machine happens to have — the difference the CI runner would otherwise be
/// the only one to notice.
const FIXTURE_MARKER: &str = "not-a-real-datasheet-L15";

/// A `Device` library with a two-pin `R`, in the KiCAD 10 symdir layout —
/// enough for a placement to resolve on a machine with no KiCAD installed,
/// which is every CI runner.
fn stub_symbol_library() -> tempfile::TempDir {
    let libdir = tempfile::tempdir().expect("tempdir");
    let symdir = libdir.path().join("Device.kicad_symdir");
    std::fs::create_dir_all(&symdir).expect("the symdir is creatable");
    std::fs::write(
        symdir.join("R.kicad_sym"),
        format!("(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"R\"\n\t\t(property \"Reference\" \"R\" (at 0 0 0))\n\t\t(property \"Value\" \"R\" (at 0 0 0))\n\t\t(property \"Datasheet\" \"{FIXTURE_MARKER}\" (at 0 0 0))\n\t\t(symbol \"R_0_1\"\n\t\t\t(pin passive line (at 0 3.81 270) (length 1.27)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"1\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t\t(pin passive line (at 0 -3.81 90) (length 1.27)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"2\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t)\n\t)\n)\n"),
    )
    .expect("the fixture symbol is writable");
    libdir
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
    let symbols = stub_symbol_library();
    let mut p = McpProcess::spawn_with_symbols(symbols.path());
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
    // And it resolved through the fixture, not through a KiCAD this machine
    // happens to have installed: without this the test proves nothing on a
    // developer box and fails on every runner.
    let written = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        written.contains(FIXTURE_MARKER),
        "the placed symbol did not come from the fixture library"
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

/// The summary belongs in the reply; the item-by-item detail belongs behind a
/// handle. A reviewer follows `kicad://diff/N` once; everyone else pays a URI.
#[test]
fn the_change_detail_lives_behind_a_handle_rather_than_in_the_reply() {
    let scratch = Scratch::new("evhandle");
    let mut p = McpProcess::spawn();

    let before = p.request("resources/list", json!({}));
    assert_eq!(
        before["result"]["resources"].as_array().map(Vec::len),
        Some(0),
        "nothing has run yet, so there is nothing to fetch: {before:#?}"
    );

    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "ev"}}
        ]}),
    ));
    let handle = created["diff"]["evidence"]
        .as_str()
        .unwrap_or_else(|| panic!("a change must carry a handle to its detail: {created:#?}"))
        .to_string();
    assert!(
        handle.starts_with("kicad://diff/"),
        "handle must be resolvable: {handle}"
    );

    let listed = p.request("resources/list", json!({}));
    let entry = listed["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["uri"].as_str() == Some(handle.as_str()))
        .unwrap_or_else(|| panic!("the handle must be listed: {listed:#?}"));
    assert_eq!(
        entry["description"].as_str(),
        Some("document +3"),
        "a listing must say what the body is, so it can be skipped: {entry:#?}"
    );

    let read = p.request("resources/read", json!({ "uri": handle }));
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read must return the body: {read:#?}"));
    let pack: Value = serde_json::from_str(text).unwrap();
    assert_eq!(pack["count"].as_u64(), Some(3), "{pack:#?}");
    assert_eq!(
        pack["changes"].as_array().map(Vec::len),
        Some(3),
        "the unbounded list is what the handle is for: {pack:#?}"
    );
    assert!(
        pack["documents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str() == Some("ev.kicad_sch")),
        "the pack names the documents it describes: {pack:#?}"
    );

    // An invented handle must fail loudly. Returning an empty body would read
    // as "the batch changed nothing", which is the opposite of the truth.
    let bad = p.request("resources/read", json!({"uri": "kicad://diff/9999"}));
    let message = bad["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("unknown_handle"),
        "an unknown handle must be an error with a stable code: {bad:#?}"
    );
}

/// The objective outlives the conversation, and a batch files itself under it
/// without being asked to. The point is that the revisions, the evidence handle
/// and the failure end up in the record because the batch produced them — not
/// because a model remembered to copy them across.
#[test]
fn a_task_collects_what_its_batches_did() {
    let scratch = Scratch::new("taskstate");
    let mut p = McpProcess::spawn();

    let started = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "start_task", "args": {
                "objective": "Build the divider and keep ERC clean",
                "constraints": ["do not touch the RF section"],
                "success_criteria": ["ERC = 0"]
            }}
        ]}),
    ));
    let task_id = started["results"][0]["result"]["task_id"]
        .as_str()
        .unwrap_or_else(|| panic!("start_task must return an id: {started:#?}"))
        .to_string();
    assert!(
        started["results"][0]["result"]["anchor"]
            .as_str()
            .is_some_and(|a| a.contains("ACTIVE TASK") && a.contains("RF section")),
        "the anchor names the hard constraint: {started:#?}"
    );

    let applied = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"task_id": task_id, "calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "ts"}}
        ]}),
    ));
    assert_eq!(
        applied["task"]["id"].as_str(),
        Some(task_id.as_str()),
        "{applied:#?}"
    );
    assert!(
        applied["task"]["anchor"].as_str().is_some(),
        "every batch under a task refreshes the anchor: {applied:#?}"
    );

    // A failure is filed too, so the next attempt can see the wall it hit.
    let failed = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"task_id": task_id, "calls": [
            {"tool": "add_schematic_component", "args": {
                "schematic": scratch.sch("ts"), "lib_id": "Nonexistent:Part", "reference": "X1"
            }}
        ]}),
    ));
    assert_eq!(failed["ok"].as_u64(), Some(0), "{failed:#?}");

    let record = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [{"tool": "get_task", "args": {"task_id": task_id}}]}),
    ));
    let task = &record["results"][0]["result"]["task"];
    assert_eq!(task["batches"].as_u64(), Some(2), "{task:#?}");
    assert!(
        task["revisions"]
            .as_object()
            .is_some_and(|r| r.keys().any(|k| k.ends_with("ts.kicad_sch"))),
        "the batch's revisions are in the record: {task:#?}"
    );
    assert!(
        task["evidence"].as_array().is_some_and(|e| e
            .iter()
            .any(|h| h.as_str().is_some_and(|h| h.starts_with("kicad://diff/")))),
        "so is the evidence handle: {task:#?}"
    );
    assert_eq!(
        task["failed_attempts"].as_array().map(Vec::len),
        Some(1),
        "and so is the failure: {task:#?}"
    );
}

/// Four meta-tools that every session pays for and most never open would be a
/// few hundred startup tokens spent on nothing. As a registry toolset they cost
/// zero and stay reachable, which is the whole argument of D20 — so it is
/// asserted rather than intended.
#[test]
fn the_task_toolset_costs_nothing_until_it_is_used() {
    let mut p = McpProcess::spawn();
    let list = p.request("tools/list", json!({}));
    let names: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for tool in ["start_task", "update_task", "get_task", "list_tasks"] {
        assert!(
            !names.contains(&tool.to_string()),
            "{tool} must not be in the startup catalogue; it is reachable through \
             kicad_invoke: {names:#?}"
        );
    }
    // Reachable all the same, without a catalogue refresh.
    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [{"tool": "list_tasks", "args": {}}]}),
    ));
    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
}

/// Same bargain as the task toolset, and a stronger case for it: `apply_plan`'s
/// schema carries an entire plan document, so as a startup tool it would be the
/// most expensive property in the catalogue.
#[test]
fn the_plan_toolset_costs_nothing_until_it_is_used() {
    let mut p = McpProcess::spawn();
    let names: Vec<String> = p.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for tool in ["preview_plan", "apply_plan"] {
        assert!(
            !names.contains(&tool.to_string()),
            "{tool} must not be in the startup catalogue: {names:#?}"
        );
    }

    // Reachable all the same, and a preview mutates nothing.
    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [{"tool": "preview_plan", "args": {"plan": {"ops": [
            {"op": "power", "with": {
                "schematic": "/p/a.kicad_sch",
                "symbols": [{"net": "GND", "x": 100.0, "y": 99.0}]
            }}
        ]}}}]}),
    ));
    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert_eq!(body["results"][0]["result"]["steps"].as_u64(), Some(1));
}

/// Same bargain again: a graph is an optimisation over tools that already
/// exist, so it must not be baseline weight on every session that never
/// queries it.
#[test]
fn the_graph_toolset_costs_nothing_until_it_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("graph_demo");
    let mut p = McpProcess::spawn();

    let names: Vec<String> = p.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for tool in ["graph_query", "graph_neighbors", "graph_stats"] {
        assert!(
            !names.contains(&tool.to_string()),
            "{tool} must not be in the startup catalogue: {names:#?}"
        );
    }

    let created = p.call_tool(
        "create_project",
        json!({"name": "graph_demo", "path": proj.to_string_lossy()}),
    );
    assert_ne!(
        created["isError"],
        json!(true),
        "create_project failed: {created}"
    );

    // Reachable all the same, without a catalogue refresh.
    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [{"tool": "graph_stats", "args": {
            "project": proj.join("graph_demo.kicad_pro").to_string_lossy()
        }}]}),
    ));
    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
}

/// The vertical slice: one MCP call carries an objective's worth of work, the
/// plan does the arithmetic, and KiCAD's own ERC says whether the result holds.
///
/// Every coordinate in this plan is deliberately **off** the 1.27 mm grid —
/// 100.0, 80.0, 76.0 — which is exactly how E6 was found: `add_power_symbol`
/// writes what it is handed, so a power symbol at a resistor's nominal position
/// lands 0.33 mm from the pin and ERC reports both ends unconnected with no tool
/// error anywhere. A plan snaps before it calls, so the same input that produced
/// six ERC errors produces none.
#[test]
fn a_plan_builds_a_divider_that_erc_passes_from_off_grid_coordinates() {
    let Some(cli) = kicad_cli_path() else {
        eprintln!("skipping: no kicad-cli found (set KONNECT_TEST_KICAD_CLI)");
        return;
    };
    let scratch = Scratch::new("plandivider");
    std::fs::write(
        scratch.dir.join("konnect.toml"),
        format!("kicad_cli = {}\n", serde_json::to_string(&cli).unwrap()),
    )
    .unwrap();
    let mut p = McpProcess::spawn_in_dir(Some(&scratch.dir));
    let sch = scratch.sch("pd");

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"verify": "auto", "calls": [{"tool": "apply_plan", "args": {"plan": {
            "plan_id": "divider",
            "ops": [
                {"op": "call", "with": {"tool": "create_project",
                    "args": {"path": scratch.path(), "name": "pd"}}},
                {"op": "place", "with": {
                    "schematic": sch,
                    "components": [
                        {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.0, "y": 80.0},
                        {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.0, "y": 95.0},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG01", "x": 100.0, "y": 76.0},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG02", "x": 100.0, "y": 99.0}
                    ]
                }},
                {"op": "power", "with": {
                    "schematic": sch,
                    "symbols": [
                        {"net": "+3V3", "x": 100.0, "y": 76.0},
                        {"net": "GND", "x": 100.0, "y": 99.0}
                    ]
                }},
                {"op": "connect", "with": {
                    "schematic": sch,
                    "connections": [{"from": "R1.2", "to": "R2.1"}]
                }},
                {"op": "label", "with": {
                    "schematic": sch,
                    "labels": [{"net": "VOUT", "x": 100.0, "y": 87.6}]
                }}
            ]
        }}}]}),
    ));

    let report = &body["results"][0]["result"];
    assert_eq!(body["ok"].as_u64(), Some(1), "{body:#?}");
    assert_eq!(
        report["ok"].as_u64(),
        report["steps"].as_u64(),
        "every step of the plan ran: {report:#?}"
    );
    assert_eq!(
        report["ops"].as_u64(),
        Some(5),
        "five operations, and more tool calls than that: {report:#?}"
    );
    assert!(
        report["steps"].as_u64().unwrap_or(0) > report["ops"].as_u64().unwrap_or(0),
        "the plan expanded rather than merely renaming calls: {report:#?}"
    );

    let erc = body["validators"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["check"].as_str() == Some("erc")))
        .unwrap_or_else(|| panic!("expected an ERC verdict: {body:#?}"));
    assert_eq!(
        erc["errors"].as_u64(),
        Some(0),
        "the plan snapped every coordinate, so nothing is 0.33 mm off its pin: {erc:#?}"
    );

    // And the batch's own machinery still applies to a plan: the change is
    // described, and the detail is behind a handle.
    assert!(
        body["diff"]["summary"].as_str().is_some(),
        "a plan is still a batch, and a batch says what it changed: {body:#?}"
    );
}

/// A plan that cannot finish never starts. The forward reference is caught by
/// the compiler, so the `create_project` ahead of it never runs either.
#[test]
fn a_plan_with_an_impossible_reference_applies_nothing() {
    let scratch = Scratch::new("planrefuse");
    let mut p = McpProcess::spawn_in_dir(Some(&scratch.dir));

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [{"tool": "apply_plan", "args": {"plan": {"ops": [
            {"id": "make", "op": "call", "with": {"tool": "create_project",
                "args": {"path": scratch.path(), "name": "pr"}}},
            {"id": "uses", "op": "call", "with": {"tool": "get_project_info",
                "args": {"project_path": "${later.path}"}}},
            {"id": "later", "op": "call", "with": {"tool": "list_schematic_components",
                "args": {"schematic": "x.kicad_sch"}}}
        ]}}}]}),
    ));

    assert_eq!(body["ok"].as_u64(), Some(0), "{body:#?}");
    assert_eq!(
        body["results"][0]["result"]["error"]["field"].as_str(),
        Some("uses"),
        "the rejection names the operation that has to change: {body:#?}"
    );
    assert!(
        !scratch.sch("pr").exists(),
        "nothing ran, so the project was never created: {body:#?}"
    );
}

/// Where `kicad-cli` really is, if it is anywhere. The tests that need a real
/// verdict are pointless without it, and hard-coding one machine's install
/// path would make them lie on every other machine.
fn kicad_cli_path() -> Option<String> {
    if let Ok(explicit) = std::env::var("KONNECT_TEST_KICAD_CLI") {
        return Some(explicit);
    }
    let exe = if cfg!(windows) {
        "kicad-cli.exe"
    } else {
        "kicad-cli"
    };
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            roots.push(std::path::PathBuf::from(&base).join("Programs\\KiCad"));
            roots.push(std::path::PathBuf::from(&base).join("KiCad"));
        }
    }
    roots.push(std::path::PathBuf::from("/usr/bin"));
    roots.push(std::path::PathBuf::from("/usr/local/bin"));
    for root in roots {
        if root.join(exe).is_file() {
            return Some(root.join(exe).to_string_lossy().into_owned());
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join("bin").join(exe);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// E4, as a test. `kicad-cli` was once absent and `run_erc` answered
/// `{"errors": 0}` — a no-op scored as a pass. A validator that could not run
/// must report that it could not run, and must never contribute a count.
#[test]
fn a_validator_that_could_not_run_is_an_error_not_a_pass() {
    let scratch = Scratch::new("verifyfail");
    std::fs::write(
        scratch.dir.join("konnect.toml"),
        "kicad_cli = \"konnect-no-such-kicad-cli\"\n",
    )
    .unwrap();
    let mut p = McpProcess::spawn_in_dir(Some(&scratch.dir));

    let body = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"verify": "auto", "calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "vf"}}
        ]}),
    ));

    let reports = body["validators"]
        .as_array()
        .unwrap_or_else(|| panic!("verify: auto must report something: {body:#?}"));
    let erc = reports
        .iter()
        .find(|r| r["check"].as_str() == Some("erc"))
        .unwrap_or_else(|| panic!("the schematic must be checked: {body:#?}"));
    assert!(
        erc["errors"].is_null() && erc["warnings"].is_null(),
        "a validator that did not run must not report counts: {erc:#?}"
    );
    assert!(
        erc["error_kind"].is_string(),
        "the failure must be classified, not narrated: {erc:#?}"
    );
    assert_eq!(
        body["ok"].as_u64(),
        Some(1),
        "the batch itself succeeded; only the check failed: {body:#?}"
    );
}

/// A misspelled `verify` must not read as a check that passed.
#[test]
fn an_unrecognised_verify_level_is_refused() {
    let scratch = Scratch::new("verifytypo");
    let mut p = McpProcess::spawn();
    let result = p.call_tool(
        "kicad_invoke",
        json!({"verify": "atuo", "calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "vt"}}
        ]}),
    );
    let body = McpProcess::tool_body(&result);
    assert_eq!(body["error"]["kind"], "invalid_argument", "{body:#?}");
    assert_eq!(body["error"]["field"], "verify", "{body:#?}");
    assert!(
        !scratch.sch("vt").exists(),
        "a refused argument must refuse the batch, not run it: {body:#?}"
    );
}

/// The diff says what moved; ERC says whether the design still holds. The
/// second batch gets a real baseline from the first, so a fix reads as a
/// finding that left rather than as a number that fell.
#[test]
fn verify_reports_a_real_erc_verdict_and_its_delta() {
    let Some(cli) = kicad_cli_path() else {
        eprintln!("skipping: no kicad-cli found (set KONNECT_TEST_KICAD_CLI)");
        return;
    };
    let scratch = Scratch::new("verifyerc");
    std::fs::write(
        scratch.dir.join("konnect.toml"),
        format!("kicad_cli = {}\n", serde_json::to_string(&cli).unwrap()),
    )
    .unwrap();
    let mut p = McpProcess::spawn_in_dir(Some(&scratch.dir));

    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"verify": "auto", "calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "ve"}}
        ]}),
    ));
    let erc = created["validators"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["check"].as_str() == Some("erc")))
        .unwrap_or_else(|| panic!("expected an ERC verdict: {created:#?}"));
    assert!(
        erc["errors"].is_u64(),
        "a validator that ran reports a count: {erc:#?}"
    );
    assert!(
        created["validators_evidence"]
            .as_str()
            .is_some_and(|u| u.starts_with("kicad://evidence/")),
        "the findings live behind a handle: {created:#?}"
    );

    // Two resistors with nothing connected to them: ERC has something to say,
    // and the baseline is the verdict the previous batch already cached.
    let sch = scratch.sch("ve");
    let placed = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"verify": "auto", "calls": [
            {"tool": "batch_place_components", "args": {"schematic": sch, "components": [
                {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25}
            ]}}
        ]}),
    ));
    let erc = placed["validators"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["check"].as_str() == Some("erc")))
        .unwrap_or_else(|| panic!("expected an ERC verdict: {placed:#?}"));
    assert!(
        erc["baseline"].is_null(),
        "the previous batch's verdict is the baseline; it must not be unknown: {erc:#?}"
    );
    assert!(
        erc["introduced"].as_u64().unwrap_or(0) > 0,
        "four unconnected pins are four new findings: {erc:#?}"
    );

    // The pack behind the handle carries the findings themselves, with the
    // stable ids the delta was computed from.
    let handle = placed["validators_evidence"].as_str().unwrap().to_string();
    let read = p.request("resources/read", json!({ "uri": handle }));
    let pack: Value =
        serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    let findings = pack["validators"][0]["findings"].as_array().unwrap();
    assert!(!findings.is_empty(), "{pack:#?}");
    let id = findings[0]["id"].as_str().unwrap();
    assert_eq!(id.len(), 12, "a finding id is a short stable digest: {id}");
    assert!(
        pack["validators"][0]["introduced"]
            .as_array()
            .is_some_and(|v| v.contains(&Value::String(id.to_string()))),
        "the delta names the ids, not only their number: {pack:#?}"
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

/// L.2.4 — the boundary the idempotency ledger deliberately does not cross,
/// and the mechanism that does.
///
/// `IdempotencyLedger` lives in the `ToolContext`, so it is per-process: a
/// second process sees any `operation_id` as `Fresh`. This pins that as
/// designed behaviour rather than leaving it to be discovered as a bug, and
/// pins the answer to "then what stops a cross-process double-apply?" right
/// next to it — `base_revisions`, which is keyed on the document's content and
/// so does not care which process moved it.
#[test]
fn an_operation_id_does_not_cross_a_process_boundary_but_base_revisions_does() {
    let scratch = Scratch::new("cross_proc");
    let mut first_process = McpProcess::spawn();
    let created = McpProcess::tool_body(&first_process.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "xp"}}
        ]}),
    ));

    let sch = scratch.sch("xp");
    let root = created["revisions_root"].as_str().map(String::from);
    let key = match &root {
        Some(_) => "xp.kicad_sch".to_string(),
        None => sch.to_string_lossy().into_owned(),
    };
    let creation_revision = created["revisions"][&key]
        .as_str()
        .unwrap_or_else(|| panic!("no revision for {key}: {created:#?}"))
        .to_string();

    let batch = json!({
        "operation_id": "op_cross_process",
        "calls": [{"tool": "add_schematic_net_label",
                   "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}]
    });

    let applied = McpProcess::tool_body(&first_process.call_tool("kicad_invoke", batch.clone()));
    assert_eq!(applied["ok"].as_u64(), Some(1), "{applied:#?}");
    assert!(
        applied["replayed"].is_null(),
        "the first run of a key is not a replay: {applied:#?}"
    );

    // A second process, same key. Its ledger is empty, so the key is `Fresh`
    // and the batch runs for real — no `replayed` marker, and no
    // `operation_in_flight` either, since the first process is done and could
    // not have told this one anything regardless.
    let mut second_process = McpProcess::spawn();
    let across = second_process.call_tool("kicad_invoke", batch.clone());
    let across_body = McpProcess::tool_body(&across);
    assert!(
        across_body["replayed"].is_null(),
        "a second process must not be able to replay the first process's key — \
         the ledger is per-ToolContext by design: {across_body:#?}"
    );
    assert_ne!(
        across_body["error"]["kind"], "operation_in_flight",
        "the two processes share no ledger, so they cannot claim against each \
         other: {across_body:#?}"
    );

    // What actually stops the cross-process double-apply is content-keyed, not
    // caller-keyed: a third process presenting the same key *and* the revision
    // the document had at creation is refused, because the document has moved
    // since — exactly as it would be for a KiCad GUI having moved it.
    let mut third_process = McpProcess::spawn();
    let mut guarded = batch;
    guarded["base_revisions"] = json!({ key: creation_revision.clone() });
    if let Some(root) = &root {
        guarded["base_revisions_root"] = json!(root);
    }
    let before_guarded = std::fs::read_to_string(&sch).unwrap();
    let refused = third_process.call_tool("kicad_invoke", guarded);
    let refused_body = McpProcess::tool_body(&refused);
    assert_eq!(refused["isError"], json!(true), "{refused_body:#?}");
    assert_eq!(refused_body["error"]["kind"], "stale_revision");
    assert_eq!(refused_body["error"]["transient"], "state");
    assert_eq!(refused_body["error"]["expected"], creation_revision);
    assert_eq!(
        std::fs::read_to_string(&sch).unwrap(),
        before_guarded,
        "a refused batch must not have run a single call"
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

// ─── L.2.2: the recovery policy a `transient` class promises ────────────────
//
// `kicad_invoke_refuses_a_stale_base_revision_without_applying_anything`
// (above) proves the refusal half of `state`. The other half — that
// reconciling (re-read, recompute, retry) actually works, and that a blind
// identical retry does not — is proved here, since a class nobody can act on
// successfully is a documentation bug wearing a test as a disguise.

/// `state`: the identical batch fails identically forever; only reconciling
/// (re-reading the revision and retrying with it) recovers. Also pins that a
/// `state` error never carries `retry_after_ms` — inviting a wait would be a
/// lie, since waiting changes nothing about a stale revision.
#[test]
fn kicad_invoke_recovers_from_a_stale_revision_by_reconciling_not_by_blind_retry() {
    let scratch = Scratch::new("state-recover");
    let mut p = McpProcess::spawn();
    let created = McpProcess::tool_body(&p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "sr"}}
        ]}),
    ));

    let sch = scratch.sch("sr");
    let root = created["revisions_root"].as_str().map(String::from);
    let key = match &root {
        Some(_) => "sr.kicad_sch".to_string(),
        None => sch.to_string_lossy().into_owned(),
    };
    let r0 = created["revisions"][&key]
        .as_str()
        .unwrap_or_else(|| panic!("no revision for {key}: {created:#?}"))
        .to_string();

    let mut stale_args = json!({
        "base_revisions": {key.clone(): r0.clone()},
        "calls": [{"tool": "add_schematic_net_label",
                   "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}}]
    });
    if let Some(root) = &root {
        stale_args["base_revisions_root"] = json!(root);
    }
    let applied = McpProcess::tool_body(&p.call_tool("kicad_invoke", stale_args.clone()));
    assert_eq!(applied["ok"].as_u64(), Some(1), "{applied:#?}");
    let after_first = std::fs::read_to_string(&sch).unwrap();

    // A blind retry of the exact same batch, against the now-stale r0, is
    // refused — twice in a row, identically. If retrying ever "worked" by
    // accident (e.g. only the first replay is checked), that would be worse
    // than never checking at all: it would teach a recovery loop that
    // hammering the same call eventually gets through.
    for attempt in 0..2 {
        let stale = p.call_tool("kicad_invoke", stale_args.clone());
        let body = McpProcess::tool_body(&stale);
        assert_eq!(
            stale["isError"],
            json!(true),
            "attempt {attempt}: {body:#?}"
        );
        assert_eq!(body["error"]["kind"], "stale_revision", "attempt {attempt}");
        assert_eq!(body["error"]["transient"], "state", "attempt {attempt}");
        assert!(
            body["error"].get("retry_after_ms").is_none(),
            "attempt {attempt}: a blind retry never fixes a stale revision, so do not invite one: {body:#?}"
        );
        assert_eq!(
            std::fs::read_to_string(&sch).unwrap(),
            after_first,
            "attempt {attempt}: a refused batch must not have touched the file"
        );
    }

    // Reconciling — re-read the current revision, rebuild the batch against
    // it — recovers. This is the other half of the policy `state` promises.
    //
    // A batch's own `revisions` map always reports absolute paths, even when
    // the batch was built with `base_revisions_root` for brevity — only
    // `create_project`'s response uses the relative form. `key` above is
    // whichever form was right for building the *request*; reading the
    // response back needs the absolute form regardless.
    let abs_key = sch.to_string_lossy().into_owned();
    let r1 = applied["revisions"][&abs_key]
        .as_str()
        .unwrap_or_else(|| panic!("no revision after the first apply: {applied:#?}"))
        .to_string();
    let mut reconciled_args = json!({
        "base_revisions": {key.clone(): r1},
        "calls": [{"tool": "add_schematic_net_label",
                   "args": {"schematic": sch, "net": "VIN", "x": 50.8, "y": 87.63}}]
    });
    if let Some(root) = &root {
        reconciled_args["base_revisions_root"] = json!(root);
    }
    let recovered = McpProcess::tool_body(&p.call_tool("kicad_invoke", reconciled_args));
    assert_eq!(
        recovered["ok"].as_u64(),
        Some(1),
        "reconciling the revision before retrying must succeed: {recovered:#?}"
    );
    assert_ne!(
        std::fs::read_to_string(&sch).unwrap(),
        after_first,
        "the reconciled retry must have actually applied"
    );
}

/// `none`: a deterministic rejection (bad arguments) is refused identically on
/// every replay, carries no `retry_after_ms`, and never touches the file —
/// there is nothing here for a retry loop to wait for or gain from.
#[test]
fn kicad_invoke_none_class_errors_stay_identical_and_free_of_a_retry_hint() {
    let scratch = Scratch::new("none-class");
    let mut p = McpProcess::spawn();
    p.call_tool(
        "kicad_invoke",
        json!({"calls": [
            {"tool": "create_project", "args": {"path": scratch.path(), "name": "nc"}}
        ]}),
    );
    let sch = scratch.sch("nc");
    let before = std::fs::read_to_string(&sch).unwrap();

    // `net` is required and missing — deterministic by construction.
    let bad_args = json!({"calls": [
        {"tool": "add_schematic_net_label", "args": {"schematic": sch, "x": 1.0, "y": 1.0}}
    ]});

    let mut bodies = Vec::new();
    for attempt in 0..2 {
        let result = p.call_tool("kicad_invoke", bad_args.clone());
        let body = McpProcess::tool_body(&result);
        // The batch envelope itself is not `isError` (D-series behavior): the
        // per-call failure lives in `results[0]`.
        let call = &body["results"][0];
        let error = &call["result"]["error"];
        assert_eq!(call["ok"], json!(false), "attempt {attempt}: {body:#?}");
        assert_eq!(
            error["kind"], "invalid_argument",
            "attempt {attempt}: {call:#?}"
        );
        assert_eq!(
            error["transient"], "none",
            "attempt {attempt}: a bad argument does not get better by waiting"
        );
        assert!(
            error.get("retry_after_ms").is_none(),
            "attempt {attempt}: nothing here to wait for: {error:#?}"
        );
        assert_eq!(
            std::fs::read_to_string(&sch).unwrap(),
            before,
            "attempt {attempt}: a failed call must not have written anything"
        );
        bodies.push(error.clone());
    }
    assert_eq!(
        bodies[0], bodies[1],
        "an identical bad request must fail identically every time"
    );
}

/// A real `std::io::Error` (missing parent directory — portable across
/// Windows/macOS/Linux, unlike a permission-denied probe) must survive
/// `SexpError` → `anyhow` and come out the other end as `Io { code }`, not as
/// an opaque `HandlerError` with `transient: none` earned by default rather
/// than by classification. A transient IO failure misclassified this way
/// would make a recovery loop give up on a call that a moment's wait — or a
/// created directory — would have let through.
///
/// `Timeout` and `Network` are not covered here: nothing in this repo can
/// provoke them without a live KiCAD IPC session (phase I is gated — this
/// machine runs KiCad 10, not 11). They are exercised by the live IPC suites
/// gated behind `#[ignore]` (decision D26), never simulated here.
#[test]
fn a_missing_parent_directory_is_classified_as_io_not_swallowed_into_handler_error() {
    let scratch = Scratch::new("io-class");
    let mut p = McpProcess::spawn();

    let ghost_sch = scratch
        .dir
        .join("does-not-exist")
        .join("also-does-not-exist.kicad_sch");

    // `add_schematic_net_label` lives in the `sch_wiring` toolset, which is
    // not part of the starter kit — load it explicitly so this is a direct
    // `tools/call` (the full structured single-call error body, with
    // `error.code`) rather than a `kicad_invoke` batch entry (which only
    // carries `error_kind`/`transient`, not `code`).
    p.call_tool("load_toolset", json!({"name": "sch_wiring"}));

    let result = p.call_tool(
        "add_schematic_net_label",
        json!({"schematic": ghost_sch, "net": "VOUT", "x": 1.0, "y": 1.0}),
    );
    let body = McpProcess::tool_body(&result);
    assert_eq!(result["isError"], json!(true), "{body:#?}");
    assert_eq!(
        body["error"]["kind"], "io",
        "a real io::Error must classify as Io, not handler_error: {body:#?}"
    );
    assert_eq!(body["error"]["code"], "not_found", "{body:#?}");
    assert_eq!(
        body["error"]["transient"], "none",
        "an unrecognised io code stays 'none' rather than a class it has not earned"
    );
}
