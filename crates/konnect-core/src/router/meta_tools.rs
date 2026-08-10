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
//! At server startup only the STARTER_KIT (`project`, `config`) is pre-loaded so
//! baseline context stays small. The LLM reads `list_toolboxes` and calls
//! `load_toolset(name)` to expose the tools it actually needs for the task.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::{CallToolResult, McpToolDescription};
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
            name: "list_toolboxes".to_string(),
            description:
                "List all available KiCAD toolsets with descriptions, categories, tool counts, \
                 and whether each is currently loaded. Only the starter kit (project, config) \
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
        "list_toolboxes" => Some(handle_list_toolboxes(ctx).await),
        "load_toolset" => Some(handle_load_toolset(args, ctx).await),
        "unload_toolset" => Some(handle_unload_toolset(args, ctx).await),
        "get_active_toolsets" => Some(handle_get_active_toolsets(ctx).await),
        "get_recent_calls" => Some(handle_get_recent_calls(args, ctx).await),
        "server_stats" => Some(handle_server_stats(ctx).await),
        _ => None,
    }
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
