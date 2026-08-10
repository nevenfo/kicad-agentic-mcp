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
                 the argument schemas. Execution stops at the first failure unless \
                 stop_on_error is false."
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
                    "stop_on_error": { "type": "boolean", "default": true }
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
                    let entry = json!({
                        "index": index,
                        "tool": name,
                        "ok": false,
                        "error_kind": "handler_error",
                        "error": e.to_string(),
                    });
                    (
                        entry,
                        CallStatus::Error,
                        Some("handler_error".to_string()),
                        0,
                    )
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

    // The envelope itself succeeded even when an inner call did not: the caller
    // needs the per-call detail to decide what to retry, and an `is_error`
    // envelope invites clients to discard the body.
    CallToolResult::json(&body)
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
