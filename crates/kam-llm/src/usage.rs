//! What a completion call cost: local token counts, time-to-first-token, and
//! throughput. Exists so the project's KPIs — `LOCAL_INPUT_TOKENS`,
//! `LOCAL_OUTPUT_TOKENS`, `TTFT_LOCAL`, `TOKENS_PER_SECOND_LOCAL` — are a
//! field read or a one-line computation on [`Usage`], never a separate
//! instrumentation pass over the call site.

use std::time::Duration;

/// Usage accounting for one [`crate::provider::Provider::complete`] call.
///
/// A backend that does not report token counts (some `llama.cpp server`
/// builds omit `usage` on a stream unless `stream_options.include_usage` is
/// honoured) leaves the counters at `0` rather than a guess — `0` is
/// distinguishable from a real small count only by the caller knowing its
/// own request was non-trivial, which is a router-level concern, not this
/// crate's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    /// Tokens the backend counted in the prompt. Feeds `LOCAL_INPUT_TOKENS`.
    pub prompt_tokens: u32,
    /// Tokens the backend generated. Feeds `LOCAL_OUTPUT_TOKENS`.
    pub completion_tokens: u32,
    /// Wall-clock time from request start to the first streamed token.
    /// `None` when the backend never sent one (empty completion, or a
    /// non-streaming path). Feeds `TTFT_LOCAL`.
    pub ttft: Option<Duration>,
    /// Wall-clock time from request start to the last streamed token.
    pub total_latency: Duration,
}

impl Usage {
    /// `LOCAL_INPUT_TOKENS` for this call.
    #[must_use]
    pub fn local_input_tokens(&self) -> u32 {
        self.prompt_tokens
    }

    /// `LOCAL_OUTPUT_TOKENS` for this call.
    #[must_use]
    pub fn local_output_tokens(&self) -> u32 {
        self.completion_tokens
    }

    /// `TTFT_LOCAL` for this call.
    #[must_use]
    pub fn ttft_local(&self) -> Option<Duration> {
        self.ttft
    }

    /// `TOKENS_PER_SECOND_LOCAL`: completion tokens generated after the first
    /// token, divided by the time spent generating them. `None` when there is
    /// nothing to divide by — no completion tokens, or no measured TTFT (so
    /// the generation-only window is unknown, not zero).
    #[must_use]
    pub fn tokens_per_second_local(&self) -> Option<f64> {
        if self.completion_tokens == 0 {
            return None;
        }
        let ttft = self.ttft?;
        let generation_time = self.total_latency.checked_sub(ttft)?;
        if generation_time.is_zero() {
            return None;
        }
        Some(f64::from(self.completion_tokens) / generation_time.as_secs_f64())
    }

    /// Total tokens (prompt + completion) this call accounts for.
    #[must_use]
    pub fn total_tokens(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_per_second_divides_generation_window_not_total() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            ttft: Some(Duration::from_millis(200)),
            total_latency: Duration::from_millis(1200),
        };
        // 1000ms of generation for 50 tokens = 50 tok/s.
        assert_eq!(usage.tokens_per_second_local(), Some(50.0));
        assert_eq!(usage.local_input_tokens(), 100);
        assert_eq!(usage.local_output_tokens(), 50);
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn no_completion_tokens_is_not_a_rate() {
        let usage = Usage {
            ttft: Some(Duration::from_millis(50)),
            total_latency: Duration::from_millis(50),
            ..Usage::default()
        };
        assert_eq!(usage.tokens_per_second_local(), None);
    }

    #[test]
    fn no_ttft_is_not_a_rate() {
        let usage = Usage {
            completion_tokens: 10,
            total_latency: Duration::from_millis(500),
            ..Usage::default()
        };
        assert_eq!(usage.tokens_per_second_local(), None);
        assert_eq!(usage.ttft_local(), None);
    }
}
