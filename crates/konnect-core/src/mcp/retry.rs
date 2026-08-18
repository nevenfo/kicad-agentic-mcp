//! One named retry policy, driven by [`TransientClass`].
//!
//! D.6.2: the server is not the agent — it must not silently retry on the
//! caller's behalf. What belongs here is not a retry *loop* but the single
//! rule every existing retry site should consult before looping: given a
//! [`TransientClass`], is retrying the identical call worth anything, and if
//! so, how long to wait first?
//!
//! The rule that matters most is `State` (and, for the same reason, `None`):
//! the world moved under the call, so the identical request will fail the
//! same way forever until something re-reads state and recomputes. That is
//! not a policy choice a call site could get right or wrong on its own — it
//! is structural here. [`decide`] returns `should_retry: false` for both
//! `State` and `None` unconditionally; nothing downstream can ask it for a
//! wait time on either.

use super::error::TransientClass;

/// What a caller should do about one failed call, given its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Whether the identical call is worth attempting again.
    pub should_retry: bool,
    /// Suggested wait before that retry. Always `None` when `should_retry`
    /// is `false` — there is nothing to wait for if nothing will be retried.
    pub wait_ms: Option<u64>,
}

impl Decision {
    const NONE: Self = Self {
        should_retry: false,
        wait_ms: None,
    };
}

/// The single retry policy. `State` never yields a retry — the world
/// changed, so the caller must re-read it and recompute, not replay the
/// same request. `None` is deterministic failure: retrying changes nothing.
/// `Lock`, `Network` and `Timeout` are worth retrying, with the wait
/// [`TransientClass::retry_after_ms`] already recommends.
#[must_use]
pub fn decide(class: TransientClass) -> Decision {
    match class {
        TransientClass::State | TransientClass::None => Decision::NONE,
        TransientClass::Lock | TransientClass::Network | TransientClass::Timeout => Decision {
            should_retry: true,
            wait_ms: class.retry_after_ms(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_driven_over_every_transient_class() {
        let cases = [
            (TransientClass::None, false, None),
            (TransientClass::Network, true, Some(1_000)),
            (TransientClass::Timeout, true, Some(1_000)),
            (TransientClass::Lock, true, Some(250)),
            (TransientClass::State, false, None),
        ];
        for (class, should_retry, wait_ms) in cases {
            let got = decide(class);
            assert_eq!(
                got.should_retry, should_retry,
                "should_retry mismatch for {class:?}"
            );
            assert_eq!(got.wait_ms, wait_ms, "wait_ms mismatch for {class:?}");
        }
    }

    /// The structural guarantee, proven directly: no `TransientClass` value
    /// classified `State` or `None` can come back with `should_retry: true`.
    /// If someone adds a new arm to [`decide`] that grants either a retry,
    /// this fails — it does not require the arm to guess it should test for
    /// this.
    #[test]
    fn state_and_none_never_retry() {
        for class in [TransientClass::State, TransientClass::None] {
            let got = decide(class);
            assert!(!got.should_retry, "{class:?} must never retry");
            assert_eq!(got.wait_ms, None, "{class:?} must never carry a wait");
        }
    }
}
