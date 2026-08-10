//! The always-visible meta-tools.
//!
//! Discovery / routing:
//!   find_capabilities(query)  — rank tools by relevance, return names + one-line summaries
//!   load_tools(names)         — expose exactly those tools, without their toolsets
//!   list_toolboxes()          — show all 18 toolsets with descriptions and load state
//!   load_toolset(name)        — activate a toolset, expose its tools in tools/list
//!   unload_toolset(name)      — deactivate a toolset, remove its tools from tools/list
//!   get_active_toolsets()     — list currently loaded toolsets
//!
//! `find_capabilities` + `load_tools` is the cheap path and
//! `list_toolboxes` + `load_toolset` is the coarse one. Both are kept: toolset
//! loading stays the right answer when a task really does sweep a domain, and
//! removing it would break every existing client and skill.
//!
//! Observability:
//!   get_recent_calls(limit?)  — last N tool calls (newest first) with timing + status
//!   server_stats()            — uptime, per-tool totals/errors, JSONL log path
//!
//! At server startup only the STARTER_KIT (`project`) and STARTER_TOOLS
//! (`load_user_config`, `get_effective_config`) are pre-loaded so baseline
//! context stays small. The LLM reaches the rest with `find_capabilities` +
//! `load_tools`, or with `list_toolboxes` + `load_toolset` for a whole domain.

use crate::mcp::error::{extract_error_kind, ToolErrorKind};
use crate::mcp::protocol::{CallToolResult, McpToolDescription};
use crate::observability::{new_call_id, unix_ms, CallRecord, CallStatus};
use crate::tools::ToolContext;
use serde_json::{json, Value};

/// Return the meta-tool MCP descriptions (always in the tools/list response).
pub fn meta_tool_descriptions() -> Vec<McpToolDescription> {
    vec![
        McpToolDescription {
            name: "find_capabilities".to_string(),
            description: "Search all 187 KiCAD tools by intent and return the best matches as \
                 name + toolset + one-line summary. This is the cheap way to build a \
                 toolbelt: describe the task in plain words ('connect two pins', \
                 'export gerbers', 'run ERC'), then pass the names you want to \
                 load_tools(). Prefer this over list_toolboxes + load_toolset, which \
                 exposes an entire domain to obtain a handful of tools."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "What you are trying to do, in plain words."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max matches to return (default 8, max 50).",
                        "default": 8
                    }
                },
                "required": ["query"]
            }),
        },
        McpToolDescription {
            name: "load_tools".to_string(),
            description: "Expose specific tools by name so they can be called, without loading \
                 their whole toolset. Names come from find_capabilities(). Unknown names \
                 are reported in not_found rather than failing the call, so one bad guess \
                 does not cost a round trip."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "names": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Tool names to expose, e.g. ['connect_pins', 'run_erc']"
                    }
                },
                "required": ["names"]
            }),
        },
        McpToolDescription {
            name: "kicad_describe".to_string(),
            description: "Return the full input schema of named tools so they can be called \
                 through kicad_invoke without ever appearing in tools/list. Names come from \
                 find_capabilities(). This is the gateway path: one response carries exactly \
                 the schemas you asked for, where load_tools makes the client re-fetch the \
                 whole catalogue instead."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "names": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Tool names to describe, e.g. ['connect_pins', 'run_erc']"
                    }
                },
                "required": ["names"]
            }),
        },
        McpToolDescription {
            name: "kicad_invoke".to_string(),
            description: "Call one or more KiCAD tools by name, in order, whether or not they \
                 are loaded. Nothing is added to tools/list, so no catalogue refresh is \
                 triggered and the batch costs only its own result. Use kicad_describe() for \
                 the argument schemas. Execution stops at the first failure, and every KiCAD \
                 file it touched is restored. The reply returns each changed file's new \
                 revision; pass those back as base_revisions so a batch built against a \
                 document the user has since edited is refused instead of applied."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "calls": {
                        "type": "array",
                        "description": "Tool calls to run in order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool": { "type": "string" },
                                "args": { "type": "object" }
                            },
                            "required": ["tool"]
                        }
                    },
                    "stop_on_error": { "type": "boolean", "default": true },
                    "atomic": {
                        "type": "boolean",
                        "description": "Undo the whole batch if any call fails. Defaults to stop_on_error."
                    },
                    "operation_id": {
                        "type": "string",
                        "description": "Idempotency key: a replay returns the first result, it does not apply twice."
                    },
                    "base_revisions": {
                        "type": "object",
                        "description": "path -> revision (or 'absent') from a previous reply. A mismatch aborts with stale_revision, having run nothing."
                    },
                    "base_revisions_root": {
                        "type": "string",
                        "description": "Root for relative base_revisions keys: pass back the revisions_root you were given."
                    },
                    "documents": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Extra files/dirs to protect when no path appears in the calls."
                    },
                    "diff": {
                        "type": "string",
                        "enum": ["none", "summary", "changes"],
                        "description": "Design diff detail. Default summary ('symbol +2, wire ~1'); changes adds a line per item."
                    }
                },
                "required": ["calls"]
            }),
        },
        McpToolDescription {
            name: "list_toolboxes".to_string(),
            description:
                "List all available KiCAD toolsets with descriptions, categories, tool counts, \
                 and whether each is currently loaded. Only the starter kit (project) \
                 is loaded at startup — call load_toolset(name) to expose additional tools \
                 in subsequent tools/list responses. Always call this first to discover what \
                 tools are available for the task."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpToolDescription {
            name: "load_toolset".to_string(),
            description:
                "Load a toolset by name so its tools appear in tools/list and can be called. \
                 Returns the names of the tools that were added; their full schemas arrive in \
                 the tools/list refresh this call triggers. Use list_toolboxes() first to \
                 see valid names. Pass an array to load several toolsets in one call -- \
                 cheaper, one tools/list refresh."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ],
                        "description": "Toolset name (e.g. 'sch_components', 'pcb_routing'), or an array of names"
                    }
                },
                "required": ["name"]
            }),
        },
        McpToolDescription {
            name: "unload_toolset".to_string(),
            description: "Unload a toolset to remove its tools from the active session. \
                 Use this to keep the tool list manageable when switching tasks. \
                 With auto_load_toolsets enabled, tools reload on use."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Toolset name to unload"
                    }
                },
                "required": ["name"]
            }),
        },
        McpToolDescription {
            name: "get_active_toolsets".to_string(),
            description:
                "Return the list of currently loaded toolsets and how many tools each provides."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpToolDescription {
            name: "get_recent_calls".to_string(),
            description:
                "Return the most recent tool calls this session (newest first) with call_id, \
                 tool name, toolset, duration, status (ok/error/not_found), and \
                 error_kind when failed. Use this to self-diagnose — e.g. 'why did the last call \
                 fail?' or 'what tools have I been running?'"
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max number of calls to return (default 20, max 100). Pass 0 for all buffered calls.",
                        "default": 20
                    }
                },
                "required": []
            }),
        },
        McpToolDescription {
            name: "server_stats".to_string(),
            description:
                "Return server uptime, total/error call counts, per-tool statistics, and the \
                 path to the JSONL call log. Good for 'what's my error rate today?' and \
                 'which tool has been slowest?'."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

/// Attempt to handle a meta-tool call. Returns `None` if the name is not a meta-tool.
pub async fn handle_meta_tool(
    name: &str,
    args: &Value,
    ctx: &std::sync::Arc<ToolContext>,
) -> Option<CallToolResult> {
    match name {
        "find_capabilities" => Some(handle_find_capabilities(args, ctx).await),
        "load_tools" => Some(handle_load_tools(args, ctx).await),
        "kicad_describe" => Some(handle_kicad_describe(args, ctx).await),
        "kicad_invoke" => Some(handle_kicad_invoke(args, ctx).await),
        "list_toolboxes" => Some(handle_list_toolboxes(ctx).await),
        "load_toolset" => Some(handle_load_toolset(args, ctx).await),
        "unload_toolset" => Some(handle_unload_toolset(args, ctx).await),
        "get_active_toolsets" => Some(handle_get_active_toolsets(ctx).await),
        "get_recent_calls" => Some(handle_get_recent_calls(args, ctx).await),
        "server_stats" => Some(handle_server_stats(ctx).await),
        _ => None,
    }
}

/// Read a deduplicated `names` array argument. A non-string entry is an error
/// rather than something to skip: silently dropping it would report a schema
/// the caller never asked about and omit the one it did.
fn require_names(args: &Value, tool: &str) -> Result<Vec<String>, CallToolResult> {
    let Some(items) = args["names"].as_array() else {
        let kind = ToolErrorKind::InvalidArgument {
            field: "names".to_string(),
            reason: "must be an array of tool names".to_string(),
        };
        return Err(CallToolResult::error_kind(
            kind,
            format!("{tool} requires names: an array of tool names"),
        ));
    };
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        match item.as_str() {
            Some(s) => names.push(s.to_string()),
            None => {
                return Err(CallToolResult::error(
                    "names array must contain only strings",
                ))
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));
    Ok(names)
}

/// `kicad_describe` — hand out input schemas without touching `tools/list`.
///
/// The catalogue is the expensive part of MCP: `load_tools` admits a tool by
/// changing what `tools/list` returns, which obliges the client to re-fetch the
/// *entire* list, startup tools included. Measured on the golden suite that
/// refresh is 2 281 tokens per task against 964 tokens of actual tool output.
/// A schema handed back as a plain result costs only itself, once.
async fn handle_kicad_describe(args: &Value, ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let names = match require_names(args, "kicad_describe") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let mut described = Vec::new();
    let mut not_found = Vec::new();
    for name in &names {
        match ctx.router.find_tool_def(name) {
            Some(def) => described.push(json!({
                "name": def.name,
                "description": def.description,
                "input_schema": def.input_schema,
            })),
            None => not_found.push(name.clone()),
        }
    }

    CallToolResult::json(&json!({
        "count": described.len(),
        "tools": described,
        "not_found": not_found,
    }))
}

/// `kicad_invoke` — run a batch of tools by name, loaded or not.
///
/// Two costs disappear at once. The catalogue never changes, so no
/// `notifications/tools/list_changed` fires and the client never re-fetches
/// `tools/list`. And a sequence that would have been N MCP round trips becomes
/// one, which is where the `MCP_CALLS per task` target comes from.
///
/// Every inner call is recorded with the shared observer under its own tool
/// name, so `get_recent_calls` / `server_stats` / the JSONL log see exactly what
/// they would have seen had the caller made the calls itself. Batching must not
/// become a way to mutate a design without an audit record.
///
/// ## What makes it a transaction rather than a loop
///
/// A batch that stops halfway used to leave the project in a state nobody
/// asked for and nobody could describe. Three guarantees fix that, in the order
/// they are applied:
///
/// 1. **`base_revisions`** — checked before anything runs. A plan compiled
///    against a document the user has since edited in KiCAD is refused, not
///    applied on top.
/// 2. **`operation_id`** — a batch that timed out in transit and is retried
///    returns its first result instead of adding the same parts again.
/// 3. **`atomic`** (defaults to `stop_on_error`) — every KiCAD document under
///    the batch's directories is captured first and written back if any call
///    fails.
///
/// The rollback is a file-level restore, not KiCAD's undo stack: the tools here
/// edit S-expression documents on disk, so putting the bytes back *is* the
/// undo. What it cannot reach is a KiCAD GUI holding the same file open — that
/// is why `base_revisions` exists as the detection half of the pair.
async fn handle_kicad_invoke(args: &Value, ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let Some(calls) = args["calls"].as_array() else {
        let kind = ToolErrorKind::InvalidArgument {
            field: "calls".to_string(),
            reason: "must be an array of {tool, args} objects".to_string(),
        };
        return CallToolResult::error_kind(
            kind,
            "kicad_invoke requires calls: [{\"tool\": \"...\", \"args\": {...}}, ...]",
        );
    };
    let stop_on_error = args["stop_on_error"].as_bool().unwrap_or(true);
    // A batch that stops at the first failure is being treated as one unit of
    // work, so it rolls back as one. A caller who passed `stop_on_error: false`
    // has said the opposite — the calls are independent and the survivors are
    // wanted — and undoing them would be the opposite of what was asked. Hence
    // the default follows `stop_on_error`; `atomic` set explicitly still wins
    // either way.
    let atomic = args["atomic"].as_bool().unwrap_or(stop_on_error);

    // Preconditions first, and before the idempotency key is claimed: a batch
    // rejected for staleness never ran, so its key must stay usable for the
    // recomputed batch that replaces it.
    if let Some(rejection) = check_base_revisions(args) {
        return rejection;
    }

    let operation_id = args["operation_id"].as_str().map(str::to_string);
    if let Some(id) = &operation_id {
        match ctx.idempotency.claim(id) {
            kam_state::Claim::Replay(mut body) => {
                if let Some(map) = body.as_object_mut() {
                    map.insert("replayed".to_string(), json!(true));
                }
                return CallToolResult::json(&body);
            }
            kam_state::Claim::InFlight => {
                let kind = ToolErrorKind::OperationInFlight {
                    operation_id: id.clone(),
                };
                return CallToolResult::error_kind(
                    kind,
                    format!(
                        "Operation '{id}' is already running. Wait for it rather than \
                         retrying — a second run would apply the same edits again."
                    ),
                );
            }
            kam_state::Claim::Fresh => {}
        }
    }

    let diff_level = DiffLevel::from_args(args);
    let guard = BatchGuard::capture(calls, args.get("documents"), atomic, diff_level);

    let mut results = Vec::with_capacity(calls.len());
    let mut ok_count = 0usize;
    let mut failed_at: Option<usize> = None;

    for (index, call) in calls.iter().enumerate() {
        let Some(name) = call["tool"].as_str() else {
            results.push(json!({
                "index": index,
                "ok": false,
                "error_kind": "invalid_argument",
                "error": "each call needs a 'tool' name",
            }));
            failed_at = Some(index);
            if stop_on_error {
                break;
            }
            continue;
        };
        let call_args = call.get("args").cloned().unwrap_or_else(|| json!({}));

        let Some(def) = ctx.router.find_tool_def(name) else {
            results.push(json!({
                "index": index,
                "tool": name,
                "ok": false,
                "error_kind": "unknown_tool",
                "error": format!("Tool '{name}' does not exist. Use find_capabilities() to look it up."),
            }));
            failed_at = Some(index);
            if stop_on_error {
                break;
            }
            continue;
        };

        let call_id = new_call_id();
        let ts = unix_ms();
        let started = std::time::Instant::now();
        let args_bytes = serde_json::to_string(&call_args)
            .map(|s| s.len())
            .unwrap_or(0);

        let (entry, status, error_kind, result_bytes) =
            match (def.handler)(&call_args, ctx.clone()).await {
                Ok(result) => {
                    let bytes = result_text(&result).len();
                    let error_kind = extract_error_kind(&result);
                    let status = if result.is_error {
                        CallStatus::Error
                    } else {
                        CallStatus::Ok
                    };
                    let entry = json!({
                        "index": index,
                        "tool": name,
                        "ok": !result.is_error,
                        "result": compact_result(&result),
                    });
                    (entry, status, error_kind, bytes)
                }
                Err(e) => {
                    // Classified, not stringified: an io failure keeps a stable
                    // code the caller can branch on even though the message it
                    // carries is in the operating system's language (E9).
                    let kind = ToolErrorKind::from_anyhow(&e);
                    let short = kind.short_code();
                    let entry = json!({
                        "index": index,
                        "tool": name,
                        "ok": false,
                        "error_kind": short,
                        "transient": kind.transient_class(),
                        "error": e.to_string(),
                    });
                    (entry, CallStatus::Error, Some(short.to_string()), 0)
                }
            };

        ctx.observer
            .record(CallRecord {
                call_id,
                ts,
                tool: name.to_string(),
                toolset: ctx.router.find_toolset_for_tool(name).map(str::to_string),
                dur_ms: started.elapsed().as_millis() as u64,
                status,
                error_kind,
                args_bytes,
                result_bytes,
            })
            .await;

        let succeeded = entry["ok"].as_bool().unwrap_or(false);
        results.push(entry);
        if succeeded {
            ok_count += 1;
        } else {
            failed_at = Some(index);
            if stop_on_error {
                break;
            }
        }
    }

    let mut body = json!({
        "count": results.len(),
        "ok": ok_count,
        "results": results,
    });
    if let Some(index) = failed_at {
        body["failed_at"] = json!(index);
        let remaining = calls.len().saturating_sub(results.len());
        if stop_on_error && remaining > 0 {
            body["not_run"] = json!(remaining);
        }
    }

    guard.finish(failed_at.is_some(), &mut body);

    if let Some(id) = &operation_id {
        ctx.idempotency.complete(id, body.clone());
    }

    // The envelope itself succeeded even when an inner call did not: the caller
    // needs the per-call detail to decide what to retry, and an `is_error`
    // envelope invites clients to discard the body.
    CallToolResult::json(&body)
}

/// At most this many revisions are reported. A batch that rewrites forty sheets
/// has already told the caller what it did; forty revision tokens on top of
/// that is the sort of payload the gateway exists to avoid.
const MAX_REPORTED_REVISIONS: usize = 20;

/// Longest directory prefix shared by every path, if there is one.
fn common_dir<'a>(paths: impl Iterator<Item = &'a std::path::Path>) -> Option<std::path::PathBuf> {
    let mut shared: Option<std::path::PathBuf> = None;
    for path in paths {
        let dir = path.parent()?;
        shared = Some(match shared {
            None => dir.to_path_buf(),
            Some(current) => {
                let mut common = std::path::PathBuf::new();
                for (a, b) in current.components().zip(dir.components()) {
                    if a != b {
                        break;
                    }
                    common.push(a);
                }
                common
            }
        });
    }
    shared.filter(|p| !p.as_os_str().is_empty())
}

/// Reject the batch if any named document has moved since the plan was built.
///
/// Keys may be absolute, or relative to `base_revisions_root` — the mirror of
/// the `revisions_root` the previous batch returned, so a caller can hand back
/// exactly what it was given.
fn check_base_revisions(args: &Value) -> Option<CallToolResult> {
    let expected = args.get("base_revisions")?.as_object()?;
    let root = args
        .get("base_revisions_root")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    for (path, want) in expected {
        let resolved = match &root {
            Some(root) => root.join(path),
            None => std::path::PathBuf::from(path),
        };
        let Some(want) = want.as_str() else {
            let kind = ToolErrorKind::InvalidArgument {
                field: "base_revisions".to_string(),
                reason: format!("revision for '{path}' must be a string"),
            };
            return Some(CallToolResult::error_kind(
                kind,
                "base_revisions maps a path to a revision string, or to 'absent'",
            ));
        };
        let actual = match kam_state::DocState::read(&resolved) {
            Ok(state) => state.token(),
            Err(e) => {
                let kind = ToolErrorKind::Io {
                    code: "read_failed",
                    detail: e.to_string(),
                };
                return Some(CallToolResult::error_kind(
                    kind,
                    format!("Could not read '{path}' to check its revision"),
                ));
            }
        };
        if actual != want {
            let kind = ToolErrorKind::StaleRevision {
                path: path.clone(),
                expected: want.to_string(),
                actual: actual.clone(),
            };
            return Some(CallToolResult::error_kind(
                kind,
                format!(
                    "'{path}' is at revision {actual}, not {want}. Nothing was applied. \
                     Re-read the document and rebuild the batch — it changed underneath you."
                ),
            ));
        }
    }
    None
}

/// How much of the design diff a batch reply carries.
///
/// `done=true` is not a reviewable answer, but neither is a hundred lines of
/// change on every call. The default is one line; the detail is one argument
/// away, and the caller decides which it is paying for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLevel {
    /// Say nothing about the design.
    None,
    /// One line: `symbol +2, wire ~1`.
    Summary,
    /// The summary plus a line per changed item, bounded.
    Changes,
}

impl DiffLevel {
    fn from_args(args: &Value) -> Self {
        match args["diff"].as_str() {
            Some("none") => Self::None,
            Some("changes") => Self::Changes,
            // Anything else, including absent and misspelled, gets the default.
            // A typo silently disabling the audit trail would be the wrong way
            // to fail.
            _ => Self::Summary,
        }
    }

    fn wants_before_image(self) -> bool {
        self != Self::None
    }
}

/// Most changes fit in a handful of lines; a re-route does not. The rest lives
/// in the documents themselves, which the caller can re-read at the revisions
/// the same reply hands back.
const MAX_REPORTED_CHANGES: usize = 25;

/// The before-image a batch can be rolled back to, plus the reason there isn't
/// one when that is the case.
struct BatchGuard {
    snapshot: Option<kam_state::Snapshot>,
    /// Set when `atomic` was requested but no snapshot could be taken. The
    /// caller is told, because "I have a net" and "I appear to have a net" are
    /// the two states this must never confuse.
    unprotected: Option<String>,
    /// Whether a failure should restore the snapshot. False when the snapshot
    /// exists only to describe the change.
    rollback: bool,
    diff: DiffLevel,
}

impl BatchGuard {
    fn capture(calls: &[Value], documents: Option<&Value>, atomic: bool, diff: DiffLevel) -> Self {
        // The before-image serves two purposes now — undo and description — so
        // it is captured when either wants it, and `rollback` records which.
        if !atomic && !diff.wants_before_image() {
            return Self {
                snapshot: None,
                unprotected: None,
                rollback: false,
                diff,
            };
        }
        let roots = crate::router::batch::discover_roots(calls, documents);
        if roots.is_empty() {
            // Nothing recognisable to protect. A read-only batch lands here and
            // needs no warning; so does a mutating batch whose paths we failed
            // to see, which does — hence saying it rather than staying silent.
            return Self {
                snapshot: None,
                unprotected: atomic.then(|| "no_project_path_found".to_string()),
                rollback: atomic,
                diff,
            };
        }
        match kam_state::Snapshot::capture(&roots, kam_state::SnapshotLimits::default()) {
            Ok(snapshot) => Self {
                snapshot: Some(snapshot),
                unprotected: None,
                rollback: atomic,
                diff,
            },
            Err(e) => Self {
                snapshot: None,
                unprotected: atomic.then(|| e.code().to_string()),
                rollback: atomic,
                diff,
            },
        }
    }

    /// Roll back on failure, then report what changed.
    fn finish(self, failed: bool, body: &mut Value) {
        if let Some(reason) = self.unprotected {
            body["unprotected"] = json!(reason);
        }
        let Some(snapshot) = self.snapshot else {
            return;
        };

        if failed && self.rollback {
            let report = snapshot.restore();
            if report.is_empty() {
                // Nothing had been written yet — the failure was clean.
                return;
            }
            let mut rolled_back = json!({
                "restored": report.restored.len(),
                "deleted": report.deleted.len(),
            });
            if !report.failed.is_empty() {
                // A partial rollback is the worst state to be silent about:
                // the project is now neither before nor after.
                rolled_back["failed"] = json!(report
                    .failed
                    .iter()
                    .map(|(path, reason)| json!({
                        "path": path.to_string_lossy(),
                        "reason": reason,
                    }))
                    .collect::<Vec<_>>());
            }
            body["rolled_back"] = rolled_back;
            return;
        }

        let changed = snapshot.changed();
        if changed.is_empty() {
            return;
        }
        report_diff(&snapshot, &changed, self.diff, body);
        // Project paths are long and identical up to the filename. Factoring
        // the directory out turns four absolute Windows paths into one plus
        // four basenames — the same information for a third of the tokens.
        let reported: Vec<_> = changed.iter().take(MAX_REPORTED_REVISIONS).collect();
        let root = if reported.len() > 1 {
            common_dir(reported.iter().map(|(p, _)| p.as_path()))
        } else {
            None
        };
        let mut revisions = serde_json::Map::new();
        for (path, state) in &reported {
            let key = match &root {
                Some(root) => path
                    .strip_prefix(root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .into_owned(),
                None => path.to_string_lossy().into_owned(),
            };
            revisions.insert(key, json!(state.token()));
        }
        if let Some(root) = root {
            body["revisions_root"] = json!(root.to_string_lossy());
        }
        body["revisions"] = Value::Object(revisions);
        if changed.len() > MAX_REPORTED_REVISIONS {
            body["revisions_omitted"] = json!(changed.len() - MAX_REPORTED_REVISIONS);
        }
    }
}

/// Describe the batch in the vocabulary of the design rather than of the
/// filesystem.
///
/// Every changed document is diffed against its before image and the results
/// are folded into one answer, because a batch that edits a sheet and its
/// parent made *one* change to the project, not two.
///
/// Documents with no domain extractor (`.kicad_pro`, `.kicad_prl`) are counted
/// rather than described. Saying "2 other files changed" is honest; guessing at
/// their contents is not.
fn report_diff(
    snapshot: &kam_state::Snapshot,
    changed: &[(std::path::PathBuf, kam_state::DocState)],
    level: DiffLevel,
    body: &mut Value,
) {
    if level == DiffLevel::None {
        return;
    }

    let mut diff = kam_evidence::Diff::default();
    let mut undescribed = 0usize;
    // Documents are items too. Creating a project adds three empty files, whose
    // *contents* differ in nothing — reporting that as "no design change" would
    // describe the one batch that changed the most as the one that changed
    // nothing.
    let mut docs_before = kam_evidence::ItemSet::new();
    let mut docs_after = kam_evidence::ItemSet::new();

    for (path, _) in changed {
        let before = snapshot.before(path);
        let after = std::fs::read(path).ok();

        let name = path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned();
        if before.is_some() {
            docs_before.insert(kam_evidence::Item::new("document", &name, &name));
        }
        if after.is_some() {
            docs_after.insert(kam_evidence::Item::new("document", &name, &name));
        }

        match crate::evidence::diff_document(path, before, after.as_deref()) {
            Some(one) => diff.extend(one),
            // Modified, but in a format nothing here can read. Counted rather
            // than described, and counted only when it was already there: a
            // created one is covered by the document diff above.
            None if before.is_some() && after.is_some() => undescribed += 1,
            None => {}
        }
    }
    diff.extend(kam_evidence::Diff::compute(&docs_before, &docs_after));

    if diff.is_empty() && undescribed == 0 {
        return;
    }

    let mut out = json!({ "summary": diff.summary() });
    if level == DiffLevel::Changes && !diff.is_empty() {
        out["changes"] = json!(diff.render_lines(MAX_REPORTED_CHANGES));
    }
    if undescribed > 0 {
        out["undescribed_files"] = json!(undescribed);
    }
    body["diff"] = out;
}

/// Concatenate a result's text content. Image content carries no text and is
/// not something a batched caller can act on, so it is skipped.
fn result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            crate::mcp::protocol::ToolContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Domain handlers return JSON encoded as a string. Re-parsing it means the
/// batch response nests real JSON instead of an escaped blob — same information,
/// fewer tokens, and directly readable by the caller.
fn compact_result(result: &CallToolResult) -> Value {
    let text = result_text(result);
    serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))
}

async fn handle_find_capabilities(
    args: &Value,
    ctx: &std::sync::Arc<ToolContext>,
) -> CallToolResult {
    let query = match args["query"].as_str() {
        Some(q) if !q.trim().is_empty() => q,
        _ => {
            let kind = ToolErrorKind::InvalidArgument {
                field: "query".to_string(),
                reason: "must be a non-empty string describing the task".to_string(),
            };
            return CallToolResult::error_kind(
                kind,
                "Missing required argument: query (describe what you are trying to do)",
            );
        }
    };
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n.clamp(1, 50) as usize)
        .unwrap_or(8);

    let corpus = ctx.router.all_tools_with_toolset();
    let hits = crate::router::capability_search::search(&corpus, query, limit);

    // Which of these are already callable matters: a caller that skips
    // load_tools for an already-loaded name saves a round trip.
    let loaded: std::collections::HashSet<String> = ctx
        .router
        .active_tools()
        .await
        .into_iter()
        .map(|d| d.name.to_string())
        .collect();

    let matches: Vec<Value> = hits
        .iter()
        .map(|h| {
            json!({
                "name": h.name,
                "toolset": h.toolset,
                "summary": h.summary,
                "loaded": loaded.contains(h.name),
            })
        })
        .collect();

    let to_load: Vec<&str> = hits
        .iter()
        .filter(|h| !loaded.contains(h.name))
        .map(|h| h.name)
        .collect();

    CallToolResult::json(&json!({
        "query": query,
        "count": matches.len(),
        "matches": matches,
        "hint": if to_load.is_empty() {
            "All matches are already loaded — call them directly.".to_string()
        } else {
            format!("Call load_tools({{\"names\": {:?}}}) to expose the ones you need.", to_load)
        },
    }))
}

async fn handle_load_tools(args: &Value, ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let Some(items) = args["names"].as_array() else {
        let kind = ToolErrorKind::InvalidArgument {
            field: "names".to_string(),
            reason: "must be an array of tool names".to_string(),
        };
        return CallToolResult::error_kind(
            kind,
            "Missing required argument: names (array of strings)",
        );
    };

    let mut requested: Vec<String> = Vec::new();
    for item in items {
        match item.as_str() {
            Some(s) => requested.push(s.to_string()),
            None => return CallToolResult::error("names array must contain only strings"),
        }
    }
    let mut seen = std::collections::HashSet::new();
    requested.retain(|n| seen.insert(n.clone()));

    let mut loaded: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    for name in &requested {
        match ctx.router.load_tool(name).await {
            Some(def) => loaded.push(def.name.to_string()),
            None => not_found.push(name.clone()),
        }
    }

    // Nothing resolved: a typed error, so the observer records a kind and the
    // caller is told to go back through find_capabilities rather than retrying
    // the same names.
    if loaded.is_empty() {
        let kind = ToolErrorKind::InvalidArgument {
            field: "names".to_string(),
            reason: not_found.join(", "),
        };
        return CallToolResult::error_kind(
            kind,
            format!(
                "No tools loaded — none of these names exist: {}. \
                 Call find_capabilities(query) to get valid names.",
                not_found.join(", ")
            ),
        );
    }

    CallToolResult::json(&json!({
        "loaded": loaded,
        "tools_added": loaded.len(),
        "not_found": not_found,
    }))
}

async fn handle_list_toolboxes(ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    use std::collections::HashSet;
    let active: HashSet<String> = ctx.router.active_names().await.into_iter().collect();

    let toolsets: Vec<Value> = ctx
        .router
        .all_toolsets()
        .iter()
        .map(|t| {
            let loaded = active.contains(t.name);
            json!({
                "name": t.name,
                "description": t.description,
                "category": t.category,
                "tool_count": t.tool_count,
                "loaded": loaded,
            })
        })
        .collect();

    CallToolResult::json(&json!({
        "toolsets": toolsets,
        "total_tools": toolsets.iter()
            .filter_map(|t| t["tool_count"].as_u64())
            .sum::<u64>(),
        "loaded_count": active.len(),
        "hint": "Only loaded toolsets contribute tools to tools/list. Call load_toolset(name) \
                 to expose a toolset's tools. Call unload_toolset(name) to prune tools you no \
                 longer need (keeps context small).",
    }))
}

async fn handle_load_toolset(args: &Value, ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    match &args["name"] {
        // Legacy single-name form: `loaded` stays a string so existing callers
        // keep parsing the result the same way.
        Value::String(name) => match ctx.router.load(name).await {
            Some(tools) => {
                let tool_list: Vec<Value> = tools.iter().map(|t| json!(t.name)).collect();
                CallToolResult::json(&json!({
                    "loaded": name,
                    "tools_added": tools.len(),
                    "tools": tool_list
                }))
            }
            None => CallToolResult::error(format!(
                "Unknown toolset '{}'. Call list_toolboxes() to see valid names.",
                name
            )),
        },
        // New array form: one load, one tools/list_changed notification.
        Value::Array(arr) => {
            let mut names: Vec<String> =
                match arr.iter().map(|v| v.as_str().map(str::to_string)).collect() {
                    Some(names) => names,
                    None => return CallToolResult::error("name array must contain only strings"),
                };
            // Duplicate names in one call would double-count tools_added.
            let mut seen = std::collections::HashSet::new();
            names.retain(|n| seen.insert(n.clone()));

            let mut loaded = Vec::new();
            let mut tools_added = 0usize;
            let mut tool_list: Vec<Value> = Vec::new();
            let mut errors = Vec::new();

            for name in &names {
                match ctx.router.load(name).await {
                    Some(tools) => {
                        loaded.push(name.clone());
                        tools_added += tools.len();
                        tool_list.extend(tools.iter().map(|t| json!(t.name)));
                    }
                    None => errors.push(format!(
                        "Unknown toolset '{}'. Call list_toolboxes() to see valid names.",
                        name
                    )),
                }
            }

            // Nothing loaded at all -- a typed error so the observer keeps a kind,
            // rather than a JSON body with a manually-set is_error flag.
            if loaded.is_empty() {
                let kind = ToolErrorKind::InvalidArgument {
                    field: "name".to_string(),
                    reason: names.join(", "),
                };
                return CallToolResult::error_kind(
                    kind,
                    format!(
                        "No toolsets loaded -- all names were unknown: {}. Call list_toolboxes() to see valid names.",
                        names.join(", ")
                    ),
                );
            }

            // Partial success (some names unknown, some loaded) is not an error --
            // the caller gets what loaded plus an errors array for the rest.
            CallToolResult::json(&json!({
                "loaded": loaded,
                "tools_added": tools_added,
                "tools": tool_list,
                "errors": errors,
            }))
        }
        _ => CallToolResult::error("Missing required argument: name (string or array of strings)"),
    }
}

async fn handle_unload_toolset(args: &Value, ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let name = match args["name"].as_str() {
        Some(n) => n,
        None => return CallToolResult::error("Missing required argument: name"),
    };

    if ctx.router.unload(name).await {
        CallToolResult::text(format!("Toolset '{}' unloaded.", name))
    } else {
        CallToolResult::error(format!("Unknown toolset '{}'.", name))
    }
}

async fn handle_get_recent_calls(
    args: &Value,
    ctx: &std::sync::Arc<ToolContext>,
) -> CallToolResult {
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(20);
    let records = ctx.observer.recent(limit).await;
    let count = records.len();
    CallToolResult::json(&json!({
        "count": count,
        "limit_applied": if limit == 0 { count } else { limit },
        "calls": records,
        "hint": "Calls are ordered newest-first. Use server_stats for aggregates.",
    }))
}

async fn handle_server_stats(ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let snap = ctx.observer.snapshot().await;
    CallToolResult::json(&snap)
}

async fn handle_get_active_toolsets(ctx: &std::sync::Arc<ToolContext>) -> CallToolResult {
    let active = ctx.router.active_names().await;
    let all = ctx.router.all_toolsets();

    let result: Vec<Value> = active
        .iter()
        .filter_map(|name| {
            all.iter().find(|t| t.name == name.as_str()).map(|meta| {
                json!({
                    "name": meta.name,
                    "description": meta.description,
                    "tool_count": meta.tool_count
                })
            })
        })
        .collect();

    CallToolResult::json(&json!({
        "active_toolsets": result,
        "total_active_tools": result.iter()
            .filter_map(|t| t["tool_count"].as_u64())
            .sum::<u64>()
    }))
}
