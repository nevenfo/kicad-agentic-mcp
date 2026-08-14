//! Explicit Agent gateway and task-state-driven local supervisor.
//!
//! Direct KiCAD MCP calls never enter this crate. An Agent caller explicitly
//! selects this gateway; its router accepts only measured `NO_LLM`, `LOCAL` or
//! `ESCALATE` decisions.

#![deny(missing_docs)]

use kam_context::{BudgetLimits, Compactor, RetrievalBundle, TaskCore};
use kam_llm::{CompletionRequest, Provider, ReasoningEffort, Role, StructuredOutput};
use kam_state::{TaskError, TaskState, TaskStore};
use serde::Serialize;
use std::sync::Arc;

/// The complete, measured routing vocabulary (D40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    /// Finish deterministically without a model call.
    NoLlm,
    /// Use the configured local model only.
    Local,
    /// Refuse model execution and return structured caller evidence.
    Escalate,
}

/// Fixed local profile selected by D38.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModelProfile;

impl LocalModelProfile {
    /// The model name recorded by the benchmark decision.
    pub const MODEL: &'static str = "gpt-oss-20b";
    /// The selected reasoning effort.
    pub const REASONING_EFFORT: ReasoningEffort = ReasoningEffort::Medium;
    /// The selected context window.
    pub const CONTEXT_WINDOW_TOKENS: u32 = 32_768;
    /// Completion reserve, explicit rather than inferred from context size.
    pub const RESERVED_COMPLETION_TOKENS: u32 = 5_120;

    /// Context limits used for every local supervisor turn.
    #[must_use]
    pub fn limits() -> BudgetLimits {
        BudgetLimits::new(
            Self::CONTEXT_WINDOW_TOKENS,
            Self::RESERVED_COMPLETION_TOKENS,
        )
        .expect("fixed local profile has a valid reserve")
    }
}

/// Measured material used to build one Agent context.
#[derive(Debug, Clone)]
pub struct SupervisorInput {
    /// The durable task to run.
    pub task_id: String,
    /// Token cost of the durable task core, counted by the backend tokenizer.
    pub task_core_tokens: u32,
    /// Token cost of non-evictable system/tool material.
    pub fixed_prefix_tokens: u32,
    /// Atomic caller-ranked bundles; electrical constraints, Plan IR and
    /// geometry must be supplied together in each bundle (D42).
    pub retrieval: Vec<RetrievalBundle>,
}

/// A structured outcome safe to return across the Agent gateway.
#[derive(Debug, Clone, Serialize)]
pub struct SupervisorOutcome {
    /// Task record read from and, where applicable, updated by the loop.
    pub task: TaskState,
    /// Measured decision that controlled this turn.
    pub decision: &'static str,
    /// True only when a local provider was called.
    pub local_model_called: bool,
    /// Local usage when a provider returned successfully.
    pub usage: Option<UsageEvidence>,
    /// Untrusted local-model proposal. It is never a verified fact.
    pub proposal: Option<String>,
    /// Stable evidence for a refusal or unavailable local execution.
    pub evidence: Vec<SupervisorEvidence>,
}

/// Serializable local usage split.
#[derive(Debug, Clone, Serialize)]
pub struct UsageEvidence {
    /// Prompt tokens measured by the provider.
    pub prompt_tokens: u32,
    /// Completion tokens measured by the provider.
    pub completion_tokens: u32,
    /// Reasoning tokens already included in completion tokens.
    pub reasoning_tokens: u32,
}

/// A stable evidence item for the caller.
#[derive(Debug, Clone, Serialize)]
pub struct SupervisorEvidence {
    /// Machine-readable evidence code.
    pub code: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

/// Supervisor state is durable `TaskState`, never a conversation transcript.
pub struct Supervisor {
    tasks: Arc<TaskStore>,
    provider: Option<Arc<dyn Provider>>,
}

impl Supervisor {
    /// Builds an Agent-only supervisor. `None` deliberately disables LOCAL.
    #[must_use]
    pub fn new(tasks: Arc<TaskStore>, provider: Option<Arc<dyn Provider>>) -> Self {
        Self { tasks, provider }
    }

    /// Runs exactly one state-derived supervisor turn.
    pub async fn run(
        &self,
        decision: RoutingDecision,
        input: SupervisorInput,
    ) -> Result<SupervisorOutcome, TaskError> {
        let task = self
            .tasks
            .get(&input.task_id)
            .ok_or_else(|| TaskError::Unknown(input.task_id.clone()))?;
        match decision {
            RoutingDecision::NoLlm => Ok(SupervisorOutcome {
                task,
                decision: "NO_LLM",
                local_model_called: false,
                usage: None,
                proposal: None,
                evidence: vec![SupervisorEvidence {
                    code: "no_llm",
                    detail: "measured route completed without a model call".to_string(),
                }],
            }),
            RoutingDecision::Escalate => Ok(SupervisorOutcome {
                task,
                decision: "ESCALATE",
                local_model_called: false,
                usage: None,
                proposal: None,
                evidence: vec![SupervisorEvidence {
                    code: "escalated",
                    detail:
                        "measured route requires caller escalation; no external model contacted"
                            .to_string(),
                }],
            }),
            RoutingDecision::Local => self.run_local(task, input).await,
        }
    }

    async fn run_local(
        &self,
        task: TaskState,
        input: SupervisorInput,
    ) -> Result<SupervisorOutcome, TaskError> {
        let Some(provider) = &self.provider else {
            return Ok(SupervisorOutcome {
                task,
                decision: "LOCAL",
                local_model_called: false,
                usage: None,
                proposal: None,
                evidence: vec![SupervisorEvidence {
                    code: "local_provider_unavailable",
                    detail: "LOCAL was measured but no local Provider is configured".to_string(),
                }],
            });
        };
        let compacted = Compactor::new(LocalModelProfile::limits())
            .compact_with_retrieval(
                TaskCore::from_task(&task, input.task_core_tokens),
                input.fixed_prefix_tokens,
                &input.retrieval,
                &[],
            )
            .map_err(|error| TaskError::Unknown(format!("context_budget:{error}")))?;
        let mut messages = vec![
            kam_llm::Message::text(Role::System, "You are the local KiCAD supervisor. Return only one Plan IR JSON object; it will be compiled and executed deterministically."),
            kam_llm::Message::text(Role::User, compacted.task_core().rendered()),
        ];
        messages.extend(compacted.retrieval().iter().map(|bundle| {
            kam_llm::Message::text(
                Role::User,
                format!("RETRIEVED {}\n{}", bundle.id(), bundle.content()),
            )
        }));
        let request = CompletionRequest {
            messages,
            reasoning_effort: Some(LocalModelProfile::REASONING_EFFORT),
            structured_output: Some(StructuredOutput {
                name: "kicad_plan".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "plan_id": {"type": "string"},
                        "documents": {"type": "array", "items": {"type": "string"}},
                        "ops": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "op": {"type": "string"},
                                    "with": {"type": "object"}
                                },
                                "required": ["op"]
                            }
                        },
                        "constraints": {"type": "array", "items": {"type": "string"}},
                        "validators": {"type": "array", "items": {"type": "string"}},
                        "rollback_policy": {"type": "string"}
                    },
                    "required": ["ops"]
                }),
                strict: false,
            }),
            temperature: Some(0.2),
            ..Default::default()
        };
        let response = match provider.complete(request).await {
            Ok(response) => response,
            Err(error) => {
                return Ok(SupervisorOutcome {
                    task,
                    decision: "LOCAL",
                    local_model_called: true,
                    usage: None,
                    proposal: None,
                    evidence: vec![SupervisorEvidence {
                        code: "local_provider_error",
                        detail: error.to_string(),
                    }],
                });
            }
        };
        let reply = response.message.content.unwrap_or_default();
        let (task, ()) = self.tasks.update(&input.task_id, |state| {
            state.add_assumption(format!("local supervisor proposal: {reply}"));
            Ok(())
        })?;
        Ok(SupervisorOutcome {
            task,
            decision: "LOCAL",
            local_model_called: true,
            usage: Some(UsageEvidence {
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: response.usage.completion_tokens,
                reasoning_tokens: response.usage.reasoning_tokens,
            }),
            proposal: Some(reply),
            evidence: vec![SupervisorEvidence {
                code: "local_model",
                detail: format!(
                    "{} medium / {} tokens",
                    LocalModelProfile::MODEL,
                    LocalModelProfile::CONTEXT_WINDOW_TOKENS
                ),
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kam_llm::{CompletionResponse, FinishReason, Message, ProviderError, Usage};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct CountingProvider {
        calls: AtomicUsize,
        requests: Mutex<Vec<CompletionRequest>>,
    }

    impl CountingProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Provider for CountingProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request);
            Ok(CompletionResponse {
                message: Message::text(Role::Assistant, "checked"),
                finish_reason: FinishReason::Stop,
                usage: Usage::default(),
            })
        }
    }
    fn input(id: &str) -> SupervisorInput {
        SupervisorInput {
            task_id: id.to_string(),
            task_core_tokens: 10,
            fixed_prefix_tokens: 10,
            retrieval: vec![RetrievalBundle::new(
                "electrical-plan-geometry",
                "atomic",
                10,
            )],
        }
    }

    #[tokio::test]
    async fn escalate_never_calls_a_provider() {
        let tasks = Arc::new(TaskStore::default());
        let task = tasks.start("route supply");
        let provider = Arc::new(CountingProvider::new());
        let supervisor = Supervisor::new(tasks, Some(provider.clone()));
        let outcome = supervisor
            .run(RoutingDecision::Escalate, input(&task.id))
            .await
            .unwrap();
        assert!(!outcome.local_model_called);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.evidence[0].code, "escalated");
    }

    #[tokio::test]
    async fn local_turn_is_derived_from_task_state_and_keeps_model_claims_unverified() {
        let tasks = Arc::new(TaskStore::default());
        let task = tasks.start("route supply");
        let provider = Arc::new(CountingProvider::new());
        let supervisor = Supervisor::new(tasks.clone(), Some(provider.clone()));
        let outcome = supervisor
            .run(RoutingDecision::Local, input(&task.id))
            .await
            .unwrap();
        assert!(outcome
            .task
            .assumptions
            .iter()
            .any(|fact| fact.contains("checked")));
        assert!(outcome.task.verified_facts.is_empty());
        assert_eq!(tasks.get(&task.id).unwrap(), outcome.task);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].reasoning_effort, Some(ReasoningEffort::Medium));
        assert_eq!(requests[0].temperature, Some(0.2));
        let structured = requests[0].structured_output.as_ref().unwrap();
        assert_eq!(structured.name, "kicad_plan");
        assert_eq!(structured.schema["required"], serde_json::json!(["ops"]));
        assert!(!structured.strict);
        assert!(requests[0].messages.iter().any(|message| {
            message
                .content
                .as_deref()
                .is_some_and(|content| content.contains("RETRIEVED electrical-plan-geometry"))
        }));
        assert_eq!(LocalModelProfile::RESERVED_COMPLETION_TOKENS, 5_120);
    }
}
