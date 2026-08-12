//! A backend-agnostic abstraction over a local, tool-calling chat-completion
//! model, plus the two pieces of ground truth a router needs before it can
//! choose one: what usage a call actually cost, and what hardware is present
//! to run it on.
//!
//! This crate is intentionally small. It does not choose a model, does not
//! rank backends, and does not route between them — that is the router
//! (Phase H, later step), which depends on a model-fit benchmark that has not
//! run yet. What is here is the seam the router will sit behind: [`provider`]
//! is the trait every backend implements, [`openai_compat`] is the one
//! concrete backend both LM Studio and `llama.cpp server` share, [`usage`] is
//! what a call costs, and [`hardware`] is what the machine offers.
//!
//! Clean-room, `MIT OR Apache-2.0`, depends on no `konnect-*` type — same
//! posture as `kam-state`, `kam-plan`, `kam-evidence`, `kam-graph`
//! (plan.md, D11).

#![deny(missing_docs)]

pub mod hardware;
pub mod openai_compat;
pub mod provider;
pub mod usage;

pub use hardware::{GpuInfo, HardwareProfile, LocalBackend};
pub use openai_compat::{ConfigError, OpenAiCompatConfig, OpenAiCompatProvider};
pub use provider::{
    CompletionRequest, CompletionResponse, FinishReason, Message, Provider, ProviderError,
    ReasoningEffort, Role, StructuredOutput, ToolCall, ToolDef,
};
pub use usage::Usage;
