//! The trait every local backend implements, and the message/tool/structured-
//! output vocabulary it speaks. A caller that only knows [`Provider`] never
//! learns whether it is talking to LM Studio, `llama.cpp server`, or
//! something else — that swap is the router's job (Phase H, later step), not
//! this crate's, but this trait is the seam it swaps behind.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::usage::Usage;

/// Who a [`Message`] is attributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt / instructions.
    System,
    /// The human or calling harness.
    User,
    /// A prior model reply, possibly carrying tool calls.
    Assistant,
    /// The result of a tool call, addressed back at the model.
    Tool,
}

/// One turn in a chat completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Who this turn is attributed to.
    pub role: Role,
    /// Text content. `None` for an assistant turn that is only tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Set on a [`Role::Tool`] message: which tool call this is the result of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Set on an assistant message that invoked one or more tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl Message {
    /// A plain text message with no tool calls.
    #[must_use]
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// A tool-result message addressed back at the model.
    #[must_use]
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }
}

/// A tool the model may call, described by name, description, and a JSON
/// Schema for its arguments — the same shape MCP tool definitions already
/// use, so a caller can hand this crate a tool catalogue without translating
/// it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Tool name, as the model will reference it in a [`ToolCall`].
    pub name: String,
    /// What the tool does, in the model's context window.
    pub description: String,
    /// JSON Schema for the arguments object.
    pub parameters: Value,
}

/// A tool invocation the model asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Backend-assigned id; echoed back in the matching [`Message::tool_result`].
    pub id: String,
    /// Which [`ToolDef::name`] this call targets.
    pub name: String,
    /// Parsed arguments. The wire format carries this as a JSON-encoded
    /// string; this type holds the parsed [`Value`] so a caller never
    /// re-parses it.
    pub arguments: Value,
}

/// Constrains the reply to a JSON Schema instead of free text, so the caller
/// gets a value it can deserialize without a repair pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredOutput {
    /// Schema name, as required by the `json_schema` response-format field.
    pub name: String,
    /// The JSON Schema the reply must satisfy.
    pub schema: Value,
    /// Whether the backend should reject/repair a reply that violates the
    /// schema rather than best-effort it. Not every backend honours this.
    #[serde(default)]
    pub strict: bool,
}

/// A completion request: the whole point of going through [`Provider`] is
/// that this same request works whichever backend implements it.
#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    /// Conversation so far, oldest first.
    pub messages: Vec<Message>,
    /// Tools the model may call this turn. Empty means no tool calling.
    pub tools: Vec<ToolDef>,
    /// If set, the reply's `content` is constrained to this schema instead of
    /// being free text. Mutually meaningful with `tools`: a backend that
    /// picks a tool call does not also produce structured content.
    pub structured_output: Option<StructuredOutput>,
    /// Sampling temperature. `None` defers to the backend's default.
    pub temperature: Option<f32>,
    /// Generation cap. `None` defers to the backend's default.
    pub max_tokens: Option<u32>,
    /// Per-call request timeout override. `None` uses the provider's
    /// configured default.
    pub timeout: Option<Duration>,
}

/// Why the backend stopped generating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    /// Ordinary end of turn.
    Stop,
    /// Stopped to emit one or more tool calls.
    ToolCalls,
    /// Hit `max_tokens` before finishing.
    Length,
    /// The backend reported a reason this crate does not recognise.
    Other(String),
}

/// The result of a completion call: the reply plus what it cost.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// The model's reply turn.
    pub message: Message,
    /// Why generation stopped.
    pub finish_reason: FinishReason,
    /// Local usage accounting for this call — see [`crate::usage`].
    pub usage: Usage,
}

/// Why a [`Provider::complete`] call failed. Every variant is something the
/// caller can act on (retry, back off, surface to the user) without knowing
/// which backend is behind the trait.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Could not reach the backend at all (connection refused, DNS failure,
    /// process not running). Distinct from [`Self::Http`]: nothing answered.
    #[error("local backend unreachable: {0}")]
    Unreachable(String),
    /// The backend answered with a non-2xx HTTP status.
    #[error("local backend returned HTTP {status}: {body}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Response body, truncated by the caller if huge.
        body: String,
    },
    /// The call did not complete within its timeout.
    #[error("local backend call timed out after {0:?}")]
    Timeout(Duration),
    /// The backend answered 2xx but the body did not parse as this crate
    /// expects (malformed JSON, missing required field, un-decodable SSE
    /// frame).
    #[error("malformed response from local backend: {0}")]
    Malformed(String),
}

/// A local, tool-calling chat-completion backend.
///
/// Object-safe on purpose (`async_trait`, not native async-fn-in-trait): the
/// router (Phase H, later step) holds `Box<dyn Provider>` and swaps backends
/// without the caller's code changing.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Run one completion call.
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError>;
}
