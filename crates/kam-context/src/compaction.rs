//! Deterministic transcript compaction around durable task state.

use kam_llm::Message;
use kam_state::TaskState;
use serde_json::json;
use std::fmt;

use crate::BudgetLimits;

/// The non-evictable part of a task context.
///
/// It is rendered directly from [`TaskState`] and carries the complete
/// objective, hard constraints, success criteria and verified facts. Nothing
/// here is summarised or truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCore {
    rendered: String,
    measured_tokens: u32,
}

impl TaskCore {
    /// Renders the durable fields and attaches their caller-measured token
    /// count. Tokenisation remains backend-specific, so this crate never
    /// estimates it from bytes or characters.
    #[must_use]
    pub fn from_task(task: &TaskState, measured_tokens: u32) -> Self {
        let rendered = json!({
            "active_task": {
                "id": &task.id,
                "objective": &task.objective,
                "constraints": &task.constraints,
                "success_criteria": &task.success_criteria,
                "verified_facts": &task.verified_facts,
            }
        })
        .to_string();
        Self {
            rendered,
            measured_tokens,
        }
    }

    /// Exact JSON block placed in the compacted prompt.
    #[must_use]
    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    /// Backend-tokenised size supplied by the caller.
    #[must_use]
    pub fn measured_tokens(&self) -> u32 {
        self.measured_tokens
    }
}

/// A transcript message paired with its backend-tokenised prompt cost.
#[derive(Debug, Clone)]
pub struct TokenCountedMessage {
    /// Original message, retained without summarisation.
    pub message: Message,
    /// Tokens this message contributes when placed back in the prompt.
    pub measured_tokens: u32,
}

/// One atomic, caller-ranked retrieval result.
///
/// Related electrical, geometry and Plan IR constraints belong in one bundle
/// when they are only useful together. The compactor either includes the whole
/// bundle or skips it; it never truncates retrieved text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalBundle {
    id: String,
    content: String,
    measured_tokens: u32,
}

impl RetrievalBundle {
    /// Creates a bundle with its stable source id and backend-tokenised cost.
    #[must_use]
    pub fn new(id: impl Into<String>, content: impl Into<String>, measured_tokens: u32) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            measured_tokens,
        }
    }

    /// Stable retrieval source id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Retrieved text, retained atomically.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Backend-tokenised prompt cost.
    #[must_use]
    pub fn measured_tokens(&self) -> u32 {
        self.measured_tokens
    }
}

/// Deterministic compaction policy for one context profile.
#[derive(Debug, Clone, Copy)]
pub struct Compactor {
    limits: BudgetLimits,
}

impl Compactor {
    /// Creates a compactor using the prompt capacity left after the configured
    /// completion reserve.
    #[must_use]
    pub fn new(limits: BudgetLimits) -> Self {
        Self { limits }
    }

    /// Keeps the durable task core and the newest contiguous transcript suffix
    /// that fits. `fixed_prefix_tokens` accounts for system instructions, tool
    /// definitions and other non-evictable prompt material.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::RequiredContextExceedsBudget`] rather than
    /// truncating any durable task field.
    pub fn compact(
        &self,
        core: TaskCore,
        fixed_prefix_tokens: u32,
        transcript: &[TokenCountedMessage],
    ) -> Result<CompactedContext, CompactionError> {
        self.compact_with_retrieval(core, fixed_prefix_tokens, &[], transcript)
    }

    /// Adds caller-ranked retrieval bundles before filling the remainder with
    /// the newest transcript suffix. Bundles are considered in input order;
    /// one that does not fit is skipped without preventing a later, smaller
    /// bundle from fitting.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::RequiredContextExceedsBudget`] when fixed
    /// material and the durable task core do not fit.
    pub fn compact_with_retrieval(
        &self,
        core: TaskCore,
        fixed_prefix_tokens: u32,
        retrieval: &[RetrievalBundle],
        transcript: &[TokenCountedMessage],
    ) -> Result<CompactedContext, CompactionError> {
        let capacity = self.limits.prompt_capacity_tokens();
        let required = u64::from(fixed_prefix_tokens) + u64::from(core.measured_tokens);
        if required > u64::from(capacity) {
            return Err(CompactionError::RequiredContextExceedsBudget {
                required_tokens: required,
                prompt_capacity_tokens: capacity,
            });
        }

        let mut used = required;
        let mut retained_retrieval = Vec::new();
        let mut dropped_retrieval_bundles = 0;
        for bundle in retrieval {
            let next = used + u64::from(bundle.measured_tokens);
            if next <= u64::from(capacity) {
                used = next;
                retained_retrieval.push(bundle.clone());
            } else {
                dropped_retrieval_bundles += 1;
            }
        }

        let mut first_retained = transcript.len();
        for (index, entry) in transcript.iter().enumerate().rev() {
            let next = used + u64::from(entry.measured_tokens);
            if next > u64::from(capacity) {
                break;
            }
            used = next;
            first_retained = index;
        }

        Ok(CompactedContext {
            task_core: core,
            retrieval: retained_retrieval,
            messages: transcript[first_retained..].to_vec(),
            used_prompt_tokens: used,
            dropped_messages: first_retained,
            dropped_retrieval_bundles,
        })
    }
}

/// A compacted prompt payload with its exact accounted size.
#[derive(Debug, Clone)]
pub struct CompactedContext {
    task_core: TaskCore,
    retrieval: Vec<RetrievalBundle>,
    messages: Vec<TokenCountedMessage>,
    used_prompt_tokens: u64,
    dropped_messages: usize,
    dropped_retrieval_bundles: usize,
}

impl CompactedContext {
    /// Complete non-evictable task record.
    #[must_use]
    pub fn task_core(&self) -> &TaskCore {
        &self.task_core
    }

    /// Retained retrieval bundles in caller-ranked order.
    #[must_use]
    pub fn retrieval(&self) -> &[RetrievalBundle] {
        &self.retrieval
    }

    /// Newest contiguous transcript suffix, in original chronological order.
    #[must_use]
    pub fn messages(&self) -> &[TokenCountedMessage] {
        &self.messages
    }

    /// Fixed prefix, task core and retained-message tokens.
    #[must_use]
    pub fn used_prompt_tokens(&self) -> u64 {
        self.used_prompt_tokens
    }

    /// Number of oldest transcript messages evicted.
    #[must_use]
    pub fn dropped_messages(&self) -> usize {
        self.dropped_messages
    }

    /// Number of retrieval bundles skipped because they did not fit.
    #[must_use]
    pub fn dropped_retrieval_bundles(&self) -> usize {
        self.dropped_retrieval_bundles
    }
}

/// A compaction request that cannot preserve its durable core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionError {
    /// Fixed prompt material plus the task core exceeds prompt capacity.
    RequiredContextExceedsBudget {
        /// Tokens that cannot be evicted.
        required_tokens: u64,
        /// Prompt capacity after reserving generation space.
        prompt_capacity_tokens: u32,
    },
}

impl fmt::Display for CompactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiredContextExceedsBudget {
                required_tokens,
                prompt_capacity_tokens,
            } => write!(
                f,
                "required context uses {required_tokens} tokens but prompt capacity is {prompt_capacity_tokens}"
            ),
        }
    }
}

impl std::error::Error for CompactionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_llm::{Message, Role};

    fn task() -> TaskState {
        let mut task = TaskState::new("task_7", "route power without changing RF\nkeep this line");
        task.add_constraint("do not touch RF").unwrap();
        task.add_constraint("ERC must remain zero").unwrap();
        task.add_success_criterion("validator says PASS").unwrap();
        task.add_verified_fact("U1 pin 3 is +3V3");
        task.add_verified_fact("revision is abc123");
        task
    }

    fn message(label: &str, tokens: u32) -> TokenCountedMessage {
        TokenCountedMessage {
            message: Message::text(Role::User, label),
            measured_tokens: tokens,
        }
    }

    #[test]
    fn task_core_preserves_every_required_field_exactly() {
        let task = task();
        let core = TaskCore::from_task(&task, 200);
        let decoded: serde_json::Value = serde_json::from_str(core.rendered()).unwrap();
        let active = &decoded["active_task"];
        assert_eq!(active["objective"], task.objective);
        assert_eq!(active["constraints"], json!(task.constraints));
        assert_eq!(active["success_criteria"], json!(task.success_criteria));
        assert_eq!(active["verified_facts"], json!(task.verified_facts));
    }

    #[test]
    fn exact_boundary_keeps_newest_contiguous_suffix_in_order() {
        let limits = BudgetLimits::new(1_000, 200).unwrap();
        let transcript = vec![
            message("old", 301),
            message("middle", 200),
            message("new", 200),
        ];
        let compacted = Compactor::new(limits)
            .compact(TaskCore::from_task(&task(), 100), 300, &transcript)
            .unwrap();

        assert_eq!(compacted.used_prompt_tokens(), 800);
        assert_eq!(compacted.dropped_messages(), 1);
        let labels = compacted
            .messages()
            .iter()
            .map(|entry| entry.message.content.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["middle", "new"]);
    }

    #[test]
    fn one_token_over_required_core_is_refused_not_truncated() {
        let limits = BudgetLimits::new(1_000, 200).unwrap();
        let err = Compactor::new(limits)
            .compact(TaskCore::from_task(&task(), 501), 300, &[])
            .unwrap_err();
        assert_eq!(
            err,
            CompactionError::RequiredContextExceedsBudget {
                required_tokens: 801,
                prompt_capacity_tokens: 800,
            }
        );
    }

    #[test]
    fn oversized_newest_message_drops_the_whole_transcript_suffix() {
        let limits = BudgetLimits::new(1_000, 200).unwrap();
        let transcript = vec![message("small old", 10), message("huge newest", 601)];
        let compacted = Compactor::new(limits)
            .compact(TaskCore::from_task(&task(), 100), 100, &transcript)
            .unwrap();
        assert!(compacted.messages().is_empty());
        assert_eq!(compacted.dropped_messages(), 2);
    }

    #[test]
    fn retrieval_is_ranked_atomic_and_budget_aware() {
        let limits = BudgetLimits::new(1_000, 200).unwrap();
        let retrieval = vec![
            RetrievalBundle::new(
                "task-electrical-plan-geometry",
                "electrical: PWR_FLAG\nplan: use power op\ngeometry: pin offset 3.81",
                250,
            ),
            RetrievalBundle::new("too-large", "must remain whole", 500),
            RetrievalBundle::new("small-fallback", "grid is 1.27", 50),
        ];
        let transcript = vec![message("old", 150), message("new", 100)];
        let compacted = Compactor::new(limits)
            .compact_with_retrieval(
                TaskCore::from_task(&task(), 100),
                200,
                &retrieval,
                &transcript,
            )
            .unwrap();

        assert_eq!(compacted.used_prompt_tokens(), 700);
        assert_eq!(compacted.dropped_retrieval_bundles(), 1);
        assert_eq!(compacted.dropped_messages(), 1);
        assert_eq!(
            compacted
                .retrieval()
                .iter()
                .map(RetrievalBundle::id)
                .collect::<Vec<_>>(),
            ["task-electrical-plan-geometry", "small-fallback"]
        );
        assert!(compacted.retrieval()[0].content().contains("PWR_FLAG"));
        assert!(compacted.retrieval()[0].content().contains("power op"));
        assert!(compacted.retrieval()[0].content().contains("pin offset"));
    }
}
