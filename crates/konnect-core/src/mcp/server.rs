//! MCP server session context.

use super::protocol::*;
use crate::router::ToolRouter;
use std::sync::Arc;

pub struct McpServerState {
    pub router: Arc<ToolRouter>,
}

impl McpServerState {
    pub fn new(router: Arc<ToolRouter>) -> Self {
        McpServerState { router }
    }

    pub fn build_initialize_result() -> InitializeResult {
        InitializeResult {
            protocol_version: "2025-06-18".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                // Evidence handles are served here. Neither flag is claimed:
                // there is no per-resource subscription, and the list changes
                // silently as batches run — a client re-lists when it follows a
                // handle it was given, which is the only time it matters.
                resources: Some(serde_json::json!({
                    "subscribe": false,
                    "listChanged": false,
                })),
                ..Default::default()
            },
            server_info: ServerInfo {
                name: "konnect".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }
}
