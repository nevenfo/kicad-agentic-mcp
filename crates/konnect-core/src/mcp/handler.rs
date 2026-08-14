//! McpHandler — receives raw JSON messages from any transport and dispatches
//! to the correct MCP method handler or tool executor.

use super::error::{extract_error_kind, ToolErrorKind};
use super::protocol::*;
use super::server::McpServerState;
use crate::observability::{
    default_calls_log_path, new_call_id, unix_ms, CallObserver, CallRecord, CallStatus,
};
use crate::router::{meta_tools, ToolRouter};
use axum::response::sse::Event;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// Clone-able handle to the MCP request handler.
/// Multiple transports (STDIO + HTTP) share the same handler.
#[derive(Clone)]
pub struct McpHandler {
    ctx: Arc<crate::tools::ToolContext>,
    sse_senders: Arc<RwLock<Vec<mpsc::Sender<Event>>>>,
    /// Raw-JSON-line notification sinks for non-SSE transports (stdio). A
    /// server-initiated notification (e.g. tools/list_changed) must reach the
    /// active transport; SSE senders only cover HTTP, so stdio registers here
    /// to receive the same notifications. Without this, notifications are
    /// silently dropped on stdio — the cause of issue #19.
    notif_sinks: Arc<RwLock<Vec<mpsc::Sender<String>>>>,
    observer: CallObserver,
}

impl McpHandler {
    pub async fn new(config: crate::tools::ServerConfig) -> anyhow::Result<Self> {
        Self::new_with_agent_provider(config, None).await
    }

    /// Build the handler with an optional local provider reachable only through
    /// the explicit `kicad_agent` entry point.
    pub async fn new_with_agent_provider(
        config: crate::tools::ServerConfig,
        provider: Option<Arc<dyn kam_llm::Provider>>,
    ) -> anyhow::Result<Self> {
        let router = Arc::new(ToolRouter::new());

        // Load only the starter kit at startup so baseline `tools/list` stays small
        // (~2K tokens, not ~23K). The LLM expands on demand via `load_toolset`.
        router.load_starter_kit().await;

        let observer = CallObserver::new(Some(default_calls_log_path()));
        let ctx = Arc::new(crate::tools::ToolContext::new_with_observer_and_provider(
            config,
            router,
            observer.clone(),
            provider,
        ));

        Ok(McpHandler {
            ctx,
            sse_senders: Arc::new(RwLock::new(Vec::new())),
            notif_sinks: Arc::new(RwLock::new(Vec::new())),
            observer,
        })
    }

    /// Accessor for the `CallObserver` — used by meta-tools `get_recent_calls`
    /// and `server_stats` that live on `ToolContext`.
    pub fn observer(&self) -> &CallObserver {
        &self.observer
    }

    pub async fn register_sse_sender(&self, tx: mpsc::Sender<Event>) {
        self.sse_senders.write().await.push(tx);
    }

    /// Register a raw-JSON-line notification sink (used by the stdio transport).
    /// Each server-initiated notification is delivered here as a serialized
    /// JSON-RPC string, which the transport writes to its output stream.
    pub async fn register_notification_sink(&self, tx: mpsc::Sender<String>) {
        self.notif_sinks.write().await.push(tx);
    }

    /// Process one JSON-RPC message and return an optional response.
    /// Returns `None` for notifications (no response required).
    pub async fn handle_message(&self, msg: Value) -> Option<JsonRpcResponse> {
        // Distinguish request (has "method") from response (has "result"/"error")
        msg.get("method")?;

        let req: JsonRpcRequest = match serde_json::from_value(msg) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::error(
                    Value::Null,
                    JsonRpcError {
                        code: INVALID_REQUEST,
                        message: format!("Invalid request: {}", e),
                        data: None,
                    },
                ));
            }
        };

        let id = req.id.clone().unwrap_or(Value::Null);
        debug!("Handling method: {}", req.method);

        let result = self.dispatch(&req).await;

        match result {
            Ok(None) => None, // notification — no response
            Ok(Some(val)) => Some(JsonRpcResponse::success(id, val)),
            Err(e) => Some(JsonRpcResponse::error(
                id,
                JsonRpcError {
                    code: INTERNAL_ERROR,
                    message: e.to_string(),
                    data: None,
                },
            )),
        }
    }

    async fn dispatch(&self, req: &JsonRpcRequest) -> anyhow::Result<Option<Value>> {
        match req.method.as_str() {
            // ── Lifecycle ──────────────────────────────────────────────────
            "initialize" => {
                let result = McpServerState::build_initialize_result();
                Ok(Some(serde_json::to_value(result)?))
            }
            "notifications/initialized" => Ok(None),
            "ping" => Ok(Some(json!({}))),

            // ── Tool listing ───────────────────────────────────────────────
            "tools/list" => {
                // Meta-tools (always visible) + all domain tools (pre-loaded at startup)
                let mut tools = meta_tools::meta_tool_descriptions();
                for def in self.ctx.router.active_tools().await {
                    tools.push(def.to_mcp_description());
                }
                let result = ListToolsResult {
                    tools,
                    next_cursor: None,
                };
                Ok(Some(serde_json::to_value(result)?))
            }

            // ── Tool execution ─────────────────────────────────────────────
            "tools/call" => {
                let params: CallToolParams =
                    serde_json::from_value(req.params.clone().unwrap_or(Value::Null))?;

                let call_result = self.execute_tool(&params).await;
                Ok(Some(serde_json::to_value(call_result)?))
            }

            // ── Evidence handles ───────────────────────────────────────────
            // What a batch changed, item by item, lives here rather than in
            // the batch's reply. The reply carries `kicad://diff/N`; this is
            // where following it lands.
            "resources/list" => {
                let resources = self
                    .ctx
                    .evidence
                    .list()
                    .into_iter()
                    .map(|entry| ResourceDescription {
                        uri: entry.uri(self.ctx.evidence.scheme()),
                        name: format!("{} {}", entry.kind, entry.id),
                        description: (!entry.summary.is_empty()).then(|| entry.summary.clone()),
                        mime_type: Some(RESOURCE_MIME.to_string()),
                    })
                    .collect();
                let result = ListResourcesResult {
                    resources,
                    next_cursor: None,
                };
                Ok(Some(serde_json::to_value(result)?))
            }

            "resources/read" => {
                let params: ReadResourceParams =
                    serde_json::from_value(req.params.clone().unwrap_or(Value::Null))?;
                match self.ctx.evidence.get(&params.uri) {
                    Ok(entry) => {
                        let result = ReadResourceResult {
                            contents: vec![ResourceContents {
                                uri: entry.uri(self.ctx.evidence.scheme()),
                                mime_type: Some(RESOURCE_MIME.to_string()),
                                text: serde_json::to_string(&entry.body)?,
                            }],
                        };
                        Ok(Some(serde_json::to_value(result)?))
                    }
                    // A handle that aged out is a different failure from one
                    // that never existed, and an agent deciding whether to
                    // re-run the check needs to know which it hit.
                    Err(e) => Err(anyhow::anyhow!("{} ({})", e.message(), e.code())),
                }
            }

            // ── Unimplemented MCP methods ──────────────────────────────────
            "prompts/list" => Ok(Some(json!({ "prompts": [] }))),

            method => {
                warn!("Unknown method: {}", method);
                Err(anyhow::anyhow!("Method not found: {}", method))
            }
        }
    }

    async fn execute_tool(&self, params: &CallToolParams) -> CallToolResult {
        let args = params.arguments.clone().unwrap_or(json!({}));
        let call_id = new_call_id();
        let started = Instant::now();
        let ts = unix_ms();

        // Pre-compute the owning toolset (if any) once for the call record.
        let toolset = self
            .ctx
            .router
            .find_toolset_for_tool(&params.name)
            .map(str::to_string);

        let args_bytes = serde_json::to_string(&args).map(|s| s.len()).unwrap_or(0);

        info!(
            call_id = %call_id,
            tool = %params.name,
            toolset = toolset.as_deref().unwrap_or("-"),
            "tool_call_start"
        );

        let (result, status, error_kind) = self.dispatch_tool(&params.name, &args).await;

        let dur_ms = started.elapsed().as_millis() as u64;
        let result_bytes = result_content_bytes(&result);

        info!(
            call_id = %call_id,
            tool = %params.name,
            status = %status.as_str(),
            dur_ms = dur_ms,
            "tool_call_end"
        );

        self.observer
            .record(CallRecord {
                call_id,
                ts,
                tool: params.name.clone(),
                toolset,
                dur_ms,
                status,
                error_kind,
                args_bytes,
                result_bytes,
            })
            .await;

        result
    }

    /// Core dispatch: meta-tool → loaded domain tool → actionable error.
    /// Returns the outcome triple so `execute_tool` can record it.
    async fn dispatch_tool(
        &self,
        name: &str,
        args: &Value,
    ) -> (CallToolResult, CallStatus, Option<String>) {
        // Meta-tools always win.
        if let Some(result) = meta_tools::handle_meta_tool(name, args, &self.ctx).await {
            // Any meta-tool that changes what tools/list would return must say
            // so, or the client keeps a stale catalogue and the newly admitted
            // tools stay uncallable for the rest of the session (issue #19).
            if matches!(name, "load_toolset" | "unload_toolset" | "load_tools") && !result.is_error
            {
                self.notify_tools_list_changed().await;
            }
            let status = if result.is_error {
                CallStatus::Error
            } else {
                CallStatus::Ok
            };
            return (result, status, None);
        }

        // Loaded domain tool? If not and auto-load is enabled (opt-in, off by
        // default -- see `ServerConfig::auto_load_toolsets`), load its toolset
        // and retry in the same call instead of erroring.
        let mut tool_def = self.ctx.router.get_tool(name).await;
        if tool_def.is_none() && self.ctx.config.auto_load_toolsets {
            if let Some(toolset) = self.ctx.router.find_toolset_for_tool(name) {
                self.ctx.router.load(toolset).await;
                self.notify_tools_list_changed().await;
                tool_def = self.ctx.router.get_tool(name).await;
            }
        }

        if let Some(tool_def) = tool_def {
            return match (tool_def.handler)(args, self.ctx.clone()).await {
                Ok(result) => {
                    let status = if result.is_error {
                        CallStatus::Error
                    } else {
                        CallStatus::Ok
                    };
                    // Structured errors carry their own kind in the body; plain-text
                    // errors fall back to "handler_error" via extract_error_kind.
                    let error_kind = extract_error_kind(&result);
                    (result, status, error_kind)
                }
                Err(e) => {
                    warn!(tool = %name, error = %e, "tool handler returned anyhow::Error");
                    // Classified rather than stringified: an io failure keeps a
                    // stable `code` the caller can match on, instead of only the
                    // OS's localized prose (progress.md E9).
                    let kind = ToolErrorKind::from_anyhow(&e);
                    let short = kind.short_code().to_string();
                    (
                        CallToolResult::error_kind(kind, format!("Tool error: {}", e)),
                        CallStatus::Error,
                        Some(short),
                    )
                }
            };
        }

        // Not loaded — try to give an actionable hint.
        match self.ctx.router.find_toolset_for_tool(name) {
            Some(toolset) => {
                let kind = ToolErrorKind::ToolsetNotLoaded {
                    toolset: toolset.to_string(),
                    tool: name.to_string(),
                };
                let msg = format!(
                    "Tool '{}' is in toolset '{}' which is not currently loaded. \
                     Call load_toolset('{}') first, then retry.",
                    name, toolset, toolset
                );
                (
                    CallToolResult::error_kind(kind, msg),
                    CallStatus::NotFound,
                    Some("toolset_not_loaded".to_string()),
                )
            }
            None => {
                let kind = ToolErrorKind::UnknownTool {
                    tool: name.to_string(),
                };
                let msg = format!(
                    "Tool '{}' not found. Use list_toolboxes() to see available toolsets.",
                    name
                );
                (
                    CallToolResult::error_kind(kind, msg),
                    CallStatus::NotFound,
                    Some("unknown_tool".to_string()),
                )
            }
        }
    }

    async fn notify_tools_list_changed(&self) {
        let notification = JsonRpcNotification::new(TOOLS_LIST_CHANGED, None);
        let Ok(json) = serde_json::to_string(&notification) else {
            return;
        };

        // HTTP/SSE clients: wrap the JSON in an SSE event. (Unchanged path.)
        {
            let event = Event::default().data(json.clone());
            let mut senders = self.sse_senders.write().await;
            senders.retain(|tx| tx.try_send(event.clone()).is_ok());
        }

        // stdio (and any other raw-line transport): deliver the JSON directly.
        // try_send is non-blocking, so emitting a notification from inside
        // request handling can never block on a full channel and deadlock the
        // request that triggered it; a dropped sink is pruned like the SSE case.
        {
            let mut sinks = self.notif_sinks.write().await;
            sinks.retain(|tx| tx.try_send(json.clone()).is_ok());
        }
    }
}

/// Sum of content bytes in a `CallToolResult` — used for observability size
/// accounting. Images are counted by their (already-base64-encoded) data len,
/// which matches what the client sees over the wire.
fn result_content_bytes(result: &CallToolResult) -> usize {
    result
        .content
        .iter()
        .map(|c| match c {
            ToolContent::Text { text } => text.len(),
            ToolContent::Image { data, .. } => data.len(),
        })
        .sum()
}
