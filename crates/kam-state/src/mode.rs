//! The operating mode: how much execution risk this process is allowed to
//! take, orthogonal to which toolset is loaded.
//!
//! Loading a toolset controls *discovery* — which tool names a client can
//! see and call. It says nothing about whether calling one is safe to allow.
//! [`OperatingMode`] is the other axis: it says whether this process may
//! mutate anything on disk at all, independent of what got discovered.
//!
//! This module is clean-room (see the crate license header): it has no idea
//! what a "tool" or an "effect" is. [`OperatingMode`] is a plain fact the
//! caller carries around; mapping it onto "may this specific call run" is
//! `konnect-core`'s job (`capability::mode_allows`), because that mapping
//! needs `Effect`, which this crate does not and must not depend on.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

/// How much execution risk this process is allowed to take.
///
/// Four variants exist because the plan names four (`plan.md`, "D.8 —
/// Operating mode, orthogonal to discovery"), but only one rule is measured
/// today: [`OperatingMode::ReadOnly`] refuses a write, everything else
/// allows it. [`OperatingMode::Manufacturing`] and
/// [`OperatingMode::Experimental`] are accepted, parsed and carried end to
/// end, but behave exactly like [`OperatingMode::Write`] until a MANIFEST
/// entry exists that would let a real distinction be tested — see
/// [`OperatingMode::tier`], the single place that fact is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatingMode {
    /// No write may run — the only restriction any mode enforces today.
    /// Not the default: an unconfigured process is `Write` (see `Default`).
    ReadOnly,
    /// Today's implicit behaviour: nothing is refused. The process default.
    Write,
    /// Parses and carries through identically to `Write` — no manufacturing-
    /// specific rule is enforced yet.
    Manufacturing,
    /// Parses and carries through identically to `Write` — no experimental-
    /// specific rule is enforced yet.
    Experimental,
}

impl OperatingMode {
    /// Restrictiveness rank: lower is more restrictive. Only two ranks exist
    /// today because only one boundary (`ReadOnly` vs. the rest) has an
    /// enforced rule; a third rank should only be added alongside the check
    /// that makes it observable, not in anticipation of one.
    fn tier(self) -> u8 {
        match self {
            OperatingMode::ReadOnly => 0,
            OperatingMode::Write | OperatingMode::Manufacturing | OperatingMode::Experimental => 1,
        }
    }

    fn encode(self) -> u8 {
        match self {
            OperatingMode::ReadOnly => 0,
            OperatingMode::Write => 1,
            OperatingMode::Manufacturing => 2,
            OperatingMode::Experimental => 3,
        }
    }

    fn decode(byte: u8) -> Self {
        match byte {
            0 => OperatingMode::ReadOnly,
            2 => OperatingMode::Manufacturing,
            3 => OperatingMode::Experimental,
            // 1 and anything unexpected (there is no public way to store
            // anything else) fall back to the least surprising value.
            _ => OperatingMode::Write,
        }
    }

    /// The more restrictive of `self` and `other`. Commutative and
    /// idempotent — there is no ordering dependence to get wrong at a call
    /// site, which is what makes it safe to expose as the only public way to
    /// change a [`ModeGuard`]'s mode after construction (see `restrict_to`,
    /// D69): whichever argument is more restrictive always wins, so passing
    /// a *less* restrictive mode than the current one is a no-op, never an
    /// elevation.
    #[must_use]
    pub fn restrict_to(self, other: OperatingMode) -> OperatingMode {
        if other.tier() < self.tier() {
            other
        } else {
            self
        }
    }
}

impl Default for OperatingMode {
    /// `Write`: today's implicit, unrestricted behaviour, so a caller that
    /// never set a mode explicitly sees no regression.
    fn default() -> Self {
        OperatingMode::Write
    }
}

impl fmt::Display for OperatingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            OperatingMode::ReadOnly => "read-only",
            OperatingMode::Write => "write",
            OperatingMode::Manufacturing => "manufacturing",
            OperatingMode::Experimental => "experimental",
        };
        f.write_str(s)
    }
}

/// `KONNECT_MODE` (or any other source) named a value that is not one of the
/// four recognised modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeParseError(pub String);

impl fmt::Display for ModeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unrecognised operating mode '{}': expected one of \
             read-only, readonly, write, manufacturing, experimental",
            self.0
        )
    }
}

impl std::error::Error for ModeParseError {}

impl FromStr for OperatingMode {
    type Err = ModeParseError;

    /// Case-insensitive. `read-only` and `readonly` both mean
    /// [`OperatingMode::ReadOnly`]; every other unrecognised value is a
    /// parse error, never a silent fallback to [`OperatingMode::Write`] —
    /// the caller decides what an absent value means, this only decides
    /// what a *present* one means.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "read-only" | "readonly" => Ok(OperatingMode::ReadOnly),
            "write" => Ok(OperatingMode::Write),
            "manufacturing" => Ok(OperatingMode::Manufacturing),
            "experimental" => Ok(OperatingMode::Experimental),
            _ => Err(ModeParseError(s.to_string())),
        }
    }
}

/// A process-lifetime holder of the current [`OperatingMode`].
///
/// D69: the mode is fixed at startup and is never elevable in-session. The
/// only public mutator is [`ModeGuard::restrict_to`], which can only move
/// the held mode toward more restrictive (via
/// [`OperatingMode::restrict_to`]'s more-restrictive-wins rule) — there is
/// no method on this type, public or private, that can move it the other
/// way. A meta-tool exposed to a model can therefore never elevate the mode
/// by calling anything on this guard, because no such call exists.
#[derive(Debug)]
pub struct ModeGuard(AtomicU8);

impl ModeGuard {
    #[must_use]
    pub fn new(mode: OperatingMode) -> Self {
        ModeGuard(AtomicU8::new(mode.encode()))
    }

    /// The mode as of this call.
    #[must_use]
    pub fn current(&self) -> OperatingMode {
        OperatingMode::decode(self.0.load(Ordering::SeqCst))
    }

    /// Move the held mode to the more restrictive of its current value and
    /// `requested`. A `requested` less restrictive than the current mode
    /// changes nothing — this is the one lowering primitive D69 allows, and
    /// it cannot be used to elevate no matter what is passed in.
    pub fn restrict_to(&self, requested: OperatingMode) {
        let mut current = self.0.load(Ordering::SeqCst);
        loop {
            let next = OperatingMode::decode(current)
                .restrict_to(requested)
                .encode();
            if next == current {
                return;
            }
            match self
                .0
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

impl Default for ModeGuard {
    /// `Write`: today's implicit, unrestricted behaviour, so a `ModeGuard`
    /// nobody configured changes nothing (no regression).
    fn default() -> Self {
        ModeGuard::new(OperatingMode::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recognised_values_case_insensitively() {
        for (raw, expected) in [
            ("read-only", OperatingMode::ReadOnly),
            ("READ-ONLY", OperatingMode::ReadOnly),
            ("readonly", OperatingMode::ReadOnly),
            ("ReadOnly", OperatingMode::ReadOnly),
            ("write", OperatingMode::Write),
            ("Write", OperatingMode::Write),
            ("manufacturing", OperatingMode::Manufacturing),
            ("MANUFACTURING", OperatingMode::Manufacturing),
            ("experimental", OperatingMode::Experimental),
            ("Experimental", OperatingMode::Experimental),
        ] {
            assert_eq!(raw.parse::<OperatingMode>().unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn unrecognised_value_is_an_error() {
        assert!("verbose".parse::<OperatingMode>().is_err());
        assert!("".parse::<OperatingMode>().is_err());
    }

    #[test]
    fn restrict_to_never_elevates() {
        let guard = ModeGuard::new(OperatingMode::ReadOnly);
        guard.restrict_to(OperatingMode::Write);
        assert_eq!(guard.current(), OperatingMode::ReadOnly);
        guard.restrict_to(OperatingMode::Manufacturing);
        assert_eq!(guard.current(), OperatingMode::ReadOnly);
        guard.restrict_to(OperatingMode::Experimental);
        assert_eq!(guard.current(), OperatingMode::ReadOnly);
    }

    #[test]
    fn restrict_to_lowers_when_requested_is_more_restrictive() {
        let guard = ModeGuard::new(OperatingMode::Write);
        guard.restrict_to(OperatingMode::ReadOnly);
        assert_eq!(guard.current(), OperatingMode::ReadOnly);
    }

    #[test]
    fn restrict_to_is_a_no_op_between_the_non_read_only_variants() {
        // Manufacturing/Experimental share Write's tier (documented, not a
        // bug): none of the three can restrict-to any of the others.
        let guard = ModeGuard::new(OperatingMode::Manufacturing);
        guard.restrict_to(OperatingMode::Experimental);
        assert_eq!(guard.current(), OperatingMode::Manufacturing);
        guard.restrict_to(OperatingMode::Write);
        assert_eq!(guard.current(), OperatingMode::Manufacturing);
    }

    #[test]
    fn default_is_write_no_regression() {
        assert_eq!(ModeGuard::default().current(), OperatingMode::Write);
    }
}
