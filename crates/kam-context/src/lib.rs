//! Token budgets for one local-agent context.
//!
//! The backend's [`kam_llm::Usage`] is authoritative. In particular,
//! deliberation is already included in completion tokens and is retained as a
//! split: it must never be ignored or charged twice.

#![deny(missing_docs)]

use kam_llm::Usage;
use std::fmt;

mod compaction;

pub use compaction::{
    CompactedContext, CompactionError, Compactor, RetrievalBundle, TaskCore, TokenCountedMessage,
};

/// Immutable limits for one context window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLimits {
    context_window_tokens: u32,
    reserved_completion_tokens: u32,
}

impl BudgetLimits {
    /// Creates explicit limits. The completion reserve must fit inside the
    /// context window; no model-specific default is guessed.
    pub fn new(
        context_window_tokens: u32,
        reserved_completion_tokens: u32,
    ) -> Result<Self, BudgetError> {
        if context_window_tokens == 0 {
            return Err(BudgetError::EmptyContextWindow);
        }
        if reserved_completion_tokens > context_window_tokens {
            return Err(BudgetError::CompletionReserveExceedsWindow {
                reserved: reserved_completion_tokens,
                window: context_window_tokens,
            });
        }
        Ok(Self {
            context_window_tokens,
            reserved_completion_tokens,
        })
    }

    /// Backend context-window size.
    #[must_use]
    pub fn context_window_tokens(self) -> u32 {
        self.context_window_tokens
    }

    /// Generation space held back from the prompt, including reasoning.
    #[must_use]
    pub fn reserved_completion_tokens(self) -> u32 {
        self.reserved_completion_tokens
    }

    /// Largest prompt that still preserves the completion reserve.
    #[must_use]
    pub fn prompt_capacity_tokens(self) -> u32 {
        self.context_window_tokens - self.reserved_completion_tokens
    }
}

/// Result of comparing the last measured call with its context budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    /// The backend returned no token counts, so safety cannot be inferred.
    Unmeasured,
    /// The prompt and completion both remain inside their limits.
    WithinBudget,
    /// The call fit, but its prompt consumed the space reserved for the next
    /// completion. E.6.2 must compact before another call.
    CompactionRequired,
    /// The measured call exceeded the context window or completion reserve.
    Exceeded,
}

/// Accounting snapshot for the latest call and cumulative spend of this
/// context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSnapshot {
    /// Classification of the latest measured call.
    pub state: BudgetState,
    /// Prompt tokens from the latest call.
    pub prompt_tokens: u32,
    /// Completion tokens from the latest call, including reasoning.
    pub completion_tokens: u32,
    /// Reasoning tokens included in `completion_tokens`.
    pub reasoning_tokens: u32,
    /// Visible answer tokens (`completion_tokens - reasoning_tokens`).
    pub answer_tokens: u32,
    /// Prompt space left while preserving the completion reserve.
    pub remaining_prompt_tokens: u32,
    /// Unused part of the configured completion reserve.
    pub remaining_completion_tokens: u32,
    /// Sum of prompt tokens reported across calls in this context.
    pub cumulative_prompt_tokens: u64,
    /// Sum of completion tokens reported across calls, reasoning included.
    pub cumulative_completion_tokens: u64,
    /// Reasoning split across calls; never added to cumulative completion.
    pub cumulative_reasoning_tokens: u64,
}

/// Mutable accounting for exactly one conversation context.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    limits: BudgetLimits,
    cumulative_prompt_tokens: u64,
    cumulative_completion_tokens: u64,
    cumulative_reasoning_tokens: u64,
}

impl ContextBudget {
    /// Starts independent accounting for one context.
    #[must_use]
    pub fn new(limits: BudgetLimits) -> Self {
        Self {
            limits,
            cumulative_prompt_tokens: 0,
            cumulative_completion_tokens: 0,
            cumulative_reasoning_tokens: 0,
        }
    }

    /// The limits assigned to this context.
    #[must_use]
    pub fn limits(&self) -> BudgetLimits {
        self.limits
    }

    /// Records one real backend call and returns its boundary classification.
    /// A reasoning split larger than the completion total is rejected.
    pub fn record(&mut self, usage: Usage) -> Result<BudgetSnapshot, BudgetError> {
        let answer_tokens =
            usage
                .local_answer_tokens()
                .ok_or(BudgetError::ReasoningExceedsCompletion {
                    reasoning: usage.reasoning_tokens,
                    completion: usage.completion_tokens,
                })?;

        self.cumulative_prompt_tokens += u64::from(usage.prompt_tokens);
        self.cumulative_completion_tokens += u64::from(usage.completion_tokens);
        self.cumulative_reasoning_tokens += u64::from(usage.reasoning_tokens);

        let prompt_capacity = self.limits.prompt_capacity_tokens();
        let measured_total = u64::from(usage.prompt_tokens) + u64::from(usage.completion_tokens);
        let state = if usage.prompt_tokens == 0 && usage.completion_tokens == 0 {
            BudgetState::Unmeasured
        } else if measured_total > u64::from(self.limits.context_window_tokens)
            || usage.completion_tokens > self.limits.reserved_completion_tokens
        {
            BudgetState::Exceeded
        } else if usage.prompt_tokens > prompt_capacity {
            BudgetState::CompactionRequired
        } else {
            BudgetState::WithinBudget
        };

        Ok(BudgetSnapshot {
            state,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            answer_tokens,
            remaining_prompt_tokens: prompt_capacity.saturating_sub(usage.prompt_tokens),
            remaining_completion_tokens: self
                .limits
                .reserved_completion_tokens
                .saturating_sub(usage.completion_tokens),
            cumulative_prompt_tokens: self.cumulative_prompt_tokens,
            cumulative_completion_tokens: self.cumulative_completion_tokens,
            cumulative_reasoning_tokens: self.cumulative_reasoning_tokens,
        })
    }
}

/// Invalid limits or malformed backend accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetError {
    /// A zero-token context window is unusable.
    EmptyContextWindow,
    /// The configured completion reserve is larger than the whole window.
    CompletionReserveExceedsWindow {
        /// Requested completion reserve.
        reserved: u32,
        /// Available context window.
        window: u32,
    },
    /// The backend reported reasoning as larger than total completion.
    ReasoningExceedsCompletion {
        /// Reported reasoning tokens.
        reasoning: u32,
        /// Reported total completion tokens.
        completion: u32,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyContextWindow => write!(f, "context window must be non-zero"),
            Self::CompletionReserveExceedsWindow { reserved, window } => write!(
                f,
                "completion reserve {reserved} exceeds context window {window}"
            ),
            Self::ReasoningExceedsCompletion {
                reasoning,
                completion,
            } => write!(
                f,
                "reasoning tokens {reasoning} exceed completion tokens {completion}"
            ),
        }
    }
}

impl std::error::Error for BudgetError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(prompt: u32, completion: u32, reasoning: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            reasoning_tokens: reasoning,
            ..Usage::default()
        }
    }

    #[test]
    fn limits_reject_impossible_boundaries() {
        assert_eq!(
            BudgetLimits::new(0, 0),
            Err(BudgetError::EmptyContextWindow)
        );
        assert_eq!(
            BudgetLimits::new(100, 101),
            Err(BudgetError::CompletionReserveExceedsWindow {
                reserved: 101,
                window: 100
            })
        );
    }

    #[test]
    fn exact_prompt_and_completion_boundaries_fit() {
        let limits = BudgetLimits::new(32_768, 5_120).unwrap();
        let mut budget = ContextBudget::new(limits);
        let snapshot = budget.record(usage(27_648, 5_120, 4_584)).unwrap();
        assert_eq!(snapshot.state, BudgetState::WithinBudget);
        assert_eq!(snapshot.remaining_prompt_tokens, 0);
        assert_eq!(snapshot.remaining_completion_tokens, 0);
        assert_eq!(snapshot.answer_tokens, 536);
    }

    #[test]
    fn one_token_past_prompt_capacity_requires_compaction() {
        let mut budget = ContextBudget::new(BudgetLimits::new(32_768, 5_120).unwrap());
        let snapshot = budget.record(usage(27_649, 5_119, 4_500)).unwrap();
        assert_eq!(snapshot.state, BudgetState::CompactionRequired);
    }

    #[test]
    fn completion_reserve_and_window_overruns_are_exceeded() {
        let limits = BudgetLimits::new(10_000, 2_000).unwrap();
        let mut reserve_overrun = ContextBudget::new(limits);
        assert_eq!(
            reserve_overrun
                .record(usage(1_000, 2_001, 1_900))
                .unwrap()
                .state,
            BudgetState::Exceeded
        );

        let mut window_overrun = ContextBudget::new(limits);
        assert_eq!(
            window_overrun
                .record(usage(8_001, 2_000, 1_900))
                .unwrap()
                .state,
            BudgetState::Exceeded
        );
    }

    #[test]
    fn reasoning_is_split_not_double_charged_across_calls() {
        let mut budget = ContextBudget::new(BudgetLimits::new(32_768, 5_120).unwrap());
        budget.record(usage(2_707, 5_044, 4_584)).unwrap();
        let second = budget.record(usage(2_620, 4_451, 4_171)).unwrap();
        assert_eq!(second.answer_tokens, 280);
        assert_eq!(second.cumulative_completion_tokens, 9_495);
        assert_eq!(second.cumulative_reasoning_tokens, 8_755);
    }

    #[test]
    fn missing_counts_are_unknown_and_bad_split_is_rejected() {
        let mut budget = ContextBudget::new(BudgetLimits::new(32_768, 5_120).unwrap());
        assert_eq!(
            budget.record(Usage::default()).unwrap().state,
            BudgetState::Unmeasured
        );
        assert_eq!(
            budget.record(usage(10, 3, 4)),
            Err(BudgetError::ReasoningExceedsCompletion {
                reasoning: 4,
                completion: 3
            })
        );
    }
}
