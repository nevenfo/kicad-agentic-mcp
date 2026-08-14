//! A [`Provider`] over any server that speaks the OpenAI-compatible
//! `/v1/chat/completions` API — LM Studio and `llama.cpp server` both do,
//! and neither's port or default model is assumed here: the caller supplies
//! a full base URL and model name.
//!
//! Security posture: [`OpenAiCompatConfig::new`] refuses a non-loopback host
//! unless the caller explicitly opts out with
//! [`OpenAiCompatConfig::allow_remote`]. The project rule is no unattended
//! network exposure of a local inference backend; the opt-out exists for a
//! caller that has made its own, separate decision about that, not as a
//! default.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use crate::provider::{
    CompletionRequest, CompletionResponse, FinishReason, Message, Provider, ProviderError, Role,
    ToolCall,
};
use crate::usage::Usage;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Configuration for an [`OpenAiCompatProvider`]. No port or model is
/// defaulted — LM Studio and `llama.cpp server` do not agree on either, and
/// guessing one would silently talk to the wrong backend.
#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    /// Base URL up to and including `/v1`, e.g. `http://127.0.0.1:1234/v1`.
    pub base_url: String,
    /// Model identifier as the backend expects it in the request body.
    pub model: String,
    /// Sent as `Authorization: Bearer <key>` if set. Most local backends
    /// ignore it; kept for the ones that check for a placeholder value.
    pub api_key: Option<String>,
    /// Per-call timeout, overridable per request via
    /// [`CompletionRequest::timeout`].
    pub timeout: Duration,
}

/// Why an [`OpenAiCompatConfig`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The base URL's host is not loopback and [`OpenAiCompatConfig::allow_remote`]
    /// was not used to opt in.
    #[error("refusing non-loopback base URL {0:?}: local inference backends must not be exposed to the network (call `.allow_remote()` to override deliberately)")]
    NonLoopbackHost(String),
    /// The base URL did not parse as `scheme://host[:port][/path]`.
    #[error("could not parse a host out of base URL {0:?}")]
    UnparsableUrl(String),
}

impl OpenAiCompatConfig {
    /// Build a config, refusing a non-loopback `base_url`. Loopback = host is
    /// `localhost`, `127.0.0.1`, any `127.x.x.x`, or `::1`.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Result<Self, ConfigError> {
        let base_url = base_url.into();
        let host =
            extract_host(&base_url).ok_or_else(|| ConfigError::UnparsableUrl(base_url.clone()))?;
        if !is_loopback_host(&host) {
            return Err(ConfigError::NonLoopbackHost(base_url));
        }
        Ok(Self {
            base_url,
            model: model.into(),
            api_key: None,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Build a config without the loopback check. The caller is asserting it
    /// has made its own decision about exposing a local backend — this crate
    /// does not make that decision for it.
    #[must_use]
    pub fn allow_remote(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Set the request timeout (builder-style).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the API key (builder-style).
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }
}

fn extract_host(base_url: &str) -> Option<String> {
    let after_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?'])
        .next()
        .unwrap_or(after_scheme);
    // Strip a trailing `:port`, but not the colons inside an IPv6 literal.
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed.split(']').next().map(str::to_string);
    }
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// A [`Provider`] backed by an OpenAI-compatible `/v1/chat/completions`
/// endpoint, driven over server-sent events so [`Usage::ttft`] reflects the
/// time to the first streamed token rather than the whole call.
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    /// Build a provider from a validated config.
    #[must_use]
    pub fn new(config: OpenAiCompatConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("reqwest client with a fixed timeout and default TLS config always builds");
        Self { client, config }
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let body = wire_request(&self.config.model, &request);

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        if let Some(timeout) = request.timeout {
            req = req.timeout(timeout);
        }

        let start = Instant::now();
        let resp = req.send().await.map_err(map_reqwest_err)?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: body_text,
            });
        }

        consume_sse_stream(resp, start).await
    }
}

fn wire_request(model: &str, req: &CompletionRequest) -> Value {
    let messages: Vec<Value> = req.messages.iter().map(wire_message).collect();
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    if let Some(so) = &req.structured_output {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": so.name, "schema": so.schema, "strict": so.strict },
        });
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(m) = req.max_tokens {
        body["max_tokens"] = json!(m);
    }
    // See `CompletionRequest::reasoning_effort`: omitted, not defaulted to
    // "low" — that equivalence was measured on gpt-oss-20b, not assumed here.
    if let Some(effort) = req.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

fn wire_message(m: &Message) -> Value {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut value = json!({ "role": role });
    if let Some(content) = &m.content {
        value["content"] = json!(content);
    }
    if let Some(id) = &m.tool_call_id {
        value["tool_call_id"] = json!(id);
    }
    if !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments.to_string() },
                })
            })
            .collect();
        value["tool_calls"] = Value::Array(calls);
    }
    value
}

fn map_reqwest_err(err: reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        ProviderError::Timeout(DEFAULT_TIMEOUT)
    } else if err.is_connect() {
        ProviderError::Unreachable(err.to_string())
    } else {
        ProviderError::Malformed(err.to_string())
    }
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

async fn consume_sse_stream(
    resp: reqwest::Response,
    start: Instant,
) -> Result<CompletionResponse, ProviderError> {
    let mut byte_stream = resp.bytes_stream();
    let mut ttft: Option<Duration> = None;
    let mut content = String::new();
    let mut tool_calls: Vec<PartialToolCall> = Vec::new();
    let mut finish_reason = FinishReason::Stop;
    let mut usage = Usage::default();
    let mut buf = String::new();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(map_reqwest_err)?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            buf.drain(..=pos);

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                buf.clear();
                break;
            }
            if ttft.is_none() {
                ttft = Some(start.elapsed());
            }

            let event: StreamChunk =
                serde_json::from_str(data).map_err(|e| ProviderError::Malformed(e.to_string()))?;

            if let Some(u) = event.usage {
                usage.prompt_tokens = u.prompt_tokens;
                usage.completion_tokens = u.completion_tokens;
                usage.reasoning_tokens = u.completion_tokens_details.reasoning_tokens;
            }
            if let Some(choice) = event.choices.into_iter().next() {
                if let Some(c) = choice.delta.content {
                    content.push_str(&c);
                }
                for delta in choice.delta.tool_calls.unwrap_or_default() {
                    merge_tool_call_delta(&mut tool_calls, delta);
                }
                if let Some(fr) = choice.finish_reason {
                    finish_reason = parse_finish_reason(&fr);
                }
            }
        }
    }

    usage.ttft = ttft;
    usage.total_latency = start.elapsed();

    let resolved_tool_calls = tool_calls
        .into_iter()
        .map(|p| {
            let arguments = if p.arguments.is_empty() {
                Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&p.arguments)
                    .map_err(|e| ProviderError::Malformed(format!("tool call arguments: {e}")))?
            };
            Ok(ToolCall {
                id: p.id,
                name: p.name,
                arguments,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;

    if !resolved_tool_calls.is_empty() && matches!(finish_reason, FinishReason::Stop) {
        finish_reason = FinishReason::ToolCalls;
    }

    let message = Message {
        role: Role::Assistant,
        content: if content.is_empty() {
            None
        } else {
            Some(content)
        },
        tool_call_id: None,
        tool_calls: resolved_tool_calls,
    };

    Ok(CompletionResponse {
        message,
        finish_reason,
        usage,
    })
}

fn merge_tool_call_delta(calls: &mut Vec<PartialToolCall>, delta: WireToolCallDelta) {
    while calls.len() <= delta.index {
        calls.push(PartialToolCall::default());
    }
    let call = &mut calls[delta.index];
    if let Some(id) = delta.id {
        call.id = id;
    }
    if let Some(function) = delta.function {
        if let Some(name) = function.name {
            call.name = name;
        }
        if let Some(arguments) = function.arguments {
            call.arguments.push_str(&arguments);
        }
    }
}

fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        other => FinishReason::Other(other.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct WireToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<WireFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct WireFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    completion_tokens_details: WireCompletionTokenDetails,
}

#[derive(Debug, Default, Deserialize)]
struct WireCompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: u32,
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::{header, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::{Json, Router};
    use tokio::net::TcpListener;

    use super::*;
    use crate::provider::{CompletionRequest, Provider, Role, StructuredOutput, ToolDef};

    /// Starts a throwaway loopback server and returns its base URL
    /// (`http://127.0.0.1:PORT/v1`) plus a handle to shut it down. Ephemeral
    /// port (`:0`) so tests never collide.
    async fn spawn_server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        (format!("http://{addr}/v1"), handle)
    }

    fn sse_body(lines: &[String]) -> Response {
        let mut body = String::new();
        for line in lines {
            body.push_str("data: ");
            body.push_str(line);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            body,
        )
            .into_response()
    }

    #[tokio::test]
    async fn nominal_call_returns_content_and_usage() {
        async fn handler() -> Response {
            sse_body(&[
                json!({"choices":[{"delta":{"content":"Hel"}}]}).to_string(),
                json!({"choices":[{"delta":{"content":"lo"}, "finish_reason":"stop"}],
                       "usage":{"prompt_tokens":12,"completion_tokens":2,
                                "completion_tokens_details":{"reasoning_tokens":1}}})
                .to_string(),
            ])
        }
        let (base_url, _server) =
            spawn_server(Router::new().route("/v1/chat/completions", post(handler))).await;
        let config = OpenAiCompatConfig::new(base_url, "any-model").unwrap();
        let provider = OpenAiCompatProvider::new(config);

        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        };
        let resp = provider.complete(req).await.unwrap();

        assert_eq!(resp.message.content.as_deref(), Some("Hello"));
        assert_eq!(resp.finish_reason, FinishReason::Stop);
        assert_eq!(resp.usage.local_input_tokens(), 12);
        assert_eq!(resp.usage.local_output_tokens(), 2);
        assert_eq!(resp.usage.local_reasoning_tokens(), 1);
        assert_eq!(resp.usage.local_answer_tokens(), Some(1));
        assert!(resp.usage.ttft_local().is_some());
    }

    #[tokio::test]
    async fn tool_call_is_reassembled_from_streamed_fragments() {
        async fn handler() -> Response {
            sse_body(&[
                json!({"choices":[{"delta":{"tool_calls":[
                    {"index":0,"id":"call_1","function":{"name":"place_component","arguments":""}}
                ]}}]})
                .to_string(),
                json!({"choices":[{"delta":{"tool_calls":[
                    {"index":0,"function":{"arguments":"{\"ref\":"}}
                ]}}]})
                .to_string(),
                json!({
                    "choices": [{
                        "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"U1\"}"}}]},
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 30, "completion_tokens": 8}
                })
                .to_string(),
            ])
        }
        let (base_url, _server) =
            spawn_server(Router::new().route("/v1/chat/completions", post(handler))).await;
        let provider =
            OpenAiCompatProvider::new(OpenAiCompatConfig::new(base_url, "any-model").unwrap());

        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "place U1")],
            tools: vec![ToolDef {
                name: "place_component".into(),
                description: "place a component".into(),
                parameters: json!({"type":"object"}),
            }],
            ..Default::default()
        };
        let resp = provider.complete(req).await.unwrap();

        assert_eq!(resp.finish_reason, FinishReason::ToolCalls);
        assert_eq!(resp.message.tool_calls.len(), 1);
        let call = &resp.message.tool_calls[0];
        assert_eq!(call.id, "call_1");
        assert_eq!(call.name, "place_component");
        assert_eq!(call.arguments, json!({"ref": "U1"}));
    }

    #[tokio::test]
    async fn structured_output_request_reaches_the_backend_unmodified() {
        let captured: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));

        async fn handler(
            State(captured): State<Arc<Mutex<Option<Value>>>>,
            Json(body): Json<Value>,
        ) -> Response {
            *captured.lock().unwrap() = Some(body);
            sse_body(&[
                json!({"choices":[{"delta":{"content":"{\"ok\":true}"},"finish_reason":"stop"}],
                              "usage":{"prompt_tokens":5,"completion_tokens":3}})
                .to_string(),
            ])
        }
        let app = Router::new()
            .route("/v1/chat/completions", post(handler))
            .with_state(captured.clone());
        let (base_url, _server) = spawn_server(app).await;
        let provider =
            OpenAiCompatProvider::new(OpenAiCompatConfig::new(base_url, "any-model").unwrap());

        let schema =
            json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]});
        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "reply as json")],
            structured_output: Some(StructuredOutput {
                name: "ok_schema".into(),
                schema: schema.clone(),
                strict: true,
            }),
            ..Default::default()
        };
        let resp = provider.complete(req).await.unwrap();

        assert_eq!(resp.message.content.as_deref(), Some("{\"ok\":true}"));
        let sent = captured.lock().unwrap().clone().unwrap();
        assert_eq!(sent["response_format"]["type"], "json_schema");
        assert_eq!(sent["response_format"]["json_schema"]["name"], "ok_schema");
        assert_eq!(sent["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(sent["response_format"]["json_schema"]["strict"], true);
    }

    #[tokio::test]
    async fn http_error_status_is_reported_not_swallowed() {
        async fn handler() -> Response {
            (StatusCode::INTERNAL_SERVER_ERROR, "model not loaded").into_response()
        }
        let (base_url, _server) =
            spawn_server(Router::new().route("/v1/chat/completions", post(handler))).await;
        let provider =
            OpenAiCompatProvider::new(OpenAiCompatConfig::new(base_url, "any-model").unwrap());

        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        };
        let err = provider.complete(req).await.unwrap_err();

        match err {
            ProviderError::Http { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("model not loaded"));
            }
            other => panic!("expected ProviderError::Http, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn slow_backend_times_out() {
        async fn handler() -> Response {
            tokio::time::sleep(Duration::from_millis(500)).await;
            sse_body(&[
                json!({"choices":[{"delta":{"content":"late"},"finish_reason":"stop"}]})
                    .to_string(),
            ])
        }
        let (base_url, _server) =
            spawn_server(Router::new().route("/v1/chat/completions", post(handler))).await;
        let config = OpenAiCompatConfig::new(base_url, "any-model")
            .unwrap()
            .with_timeout(Duration::from_millis(50));
        let provider = OpenAiCompatProvider::new(config);

        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        };
        let err = provider.complete(req).await.unwrap_err();

        assert!(
            matches!(err, ProviderError::Timeout(_)),
            "expected Timeout, got {err:?}"
        );
    }

    #[tokio::test]
    async fn absent_backend_is_unreachable_not_a_panic() {
        // Bind then immediately drop the listener: the port is very likely
        // free again but nothing is listening, which is a cheap stand-in for
        // "backend not running" without depending on a specific closed port.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let config = OpenAiCompatConfig::new(format!("http://{addr}/v1"), "any-model").unwrap();
        let provider = OpenAiCompatProvider::new(config);
        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        };
        let err = provider.complete(req).await.unwrap_err();

        assert!(
            matches!(err, ProviderError::Unreachable(_)),
            "expected Unreachable, got {err:?}"
        );
    }

    #[test]
    fn reasoning_effort_is_omitted_from_the_wire_body_when_unset() {
        // E22: unset must be an absent field, not a default of "low" — that
        // equivalence was measured on gpt-oss-20b, not this crate's choice.
        let req = CompletionRequest {
            messages: vec![Message::text(Role::User, "hi")],
            ..Default::default()
        };
        let body = wire_request("any-model", &req);
        assert!(
            body.get("reasoning_effort").is_none(),
            "expected no reasoning_effort key, got {body}"
        );
    }

    #[test]
    fn reasoning_effort_is_sent_verbatim_when_set() {
        for (effort, wire) in [
            (crate::provider::ReasoningEffort::Low, "low"),
            (crate::provider::ReasoningEffort::Medium, "medium"),
            (crate::provider::ReasoningEffort::High, "high"),
        ] {
            let req = CompletionRequest {
                messages: vec![Message::text(Role::User, "hi")],
                reasoning_effort: Some(effort),
                ..Default::default()
            };
            let body = wire_request("any-model", &req);
            assert_eq!(body["reasoning_effort"], wire, "effort {effort:?}");
        }
    }

    #[test]
    fn config_refuses_non_loopback_by_default() {
        let err = OpenAiCompatConfig::new("http://192.168.1.50:1234/v1", "m").unwrap_err();
        assert!(matches!(err, ConfigError::NonLoopbackHost(_)));
    }

    #[test]
    fn config_accepts_loopback_hosts() {
        assert!(OpenAiCompatConfig::new("http://127.0.0.1:1234/v1", "m").is_ok());
        assert!(OpenAiCompatConfig::new("http://localhost:8080/v1", "m").is_ok());
        assert!(OpenAiCompatConfig::new("http://[::1]:8080/v1", "m").is_ok());
    }

    #[test]
    fn allow_remote_bypasses_the_check_explicitly() {
        let config = OpenAiCompatConfig::allow_remote("http://192.168.1.50:1234/v1", "m");
        assert_eq!(config.base_url, "http://192.168.1.50:1234/v1");
    }
}
