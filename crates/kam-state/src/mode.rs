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
/// Three ranks exist (`plan.md`, "D.8 — Operating mode, orthogonal to
/// discovery"): [`OperatingMode::ReadOnly`] refuses every write;
/// [`OperatingMode::Manufacturing`] is the design freeze — the design is
/// validated and an order is being prepared, so reads, checks and
/// fabrication outputs pass but any write that could touch a source design
/// document is refused; [`OperatingMode::Write`] and
/// [`OperatingMode::Experimental`] refuse nothing. `Experimental` is a
/// deliberate alias of `Write` — no distinct rule exists for it in this
/// repository, and none should be invented to justify the name — kept as
/// its own variant only so a caller can name the intent. See
/// [`OperatingMode::tier`] for the exact ordering this produces:
/// `ReadOnly < Manufacturing < Write == Experimental`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatingMode {
    /// No write may run — the most restrictive mode.
    /// Not the default: an unconfigured process is `Write` (see `Default`).
    ReadOnly,
    /// Today's implicit behaviour: nothing is refused. The process default.
    Write,
    /// The design freeze: the design is considered final and an order is
    /// being prepared. Reads, checks and fabrication outputs (gerbers, BOM,
    /// pick-and-place, reports) pass; any write that can touch a source
    /// design document (`.kicad_sch`, `.kicad_pcb`, `.kicad_pro`, a project
    /// library) is refused — see `capability::WriteTarget` and
    /// `capability::mode_allows`, which encode that distinction. Stricter
    /// than `Write`, looser than `ReadOnly`.
    Manufacturing,
    /// A deliberate alias of `Write`: parses and carries through identically
    /// to it, on purpose, because no distinct experimental-mode rule exists
    /// in this repository yet. Kept as its own variant so intent is
    /// nameable, not because behaviour differs.
    Experimental,
}

impl OperatingMode {
    /// Restrictiveness rank: lower is more restrictive.
    /// `ReadOnly` (0) < `Manufacturing` (1) < `Write` == `Experimental` (2).
    /// `Manufacturing` sits strictly between the two: it is the design
    /// freeze, a rule enforced by `capability::mode_allows` on
    /// `WriteTarget::DesignDocument`, not a synonym for either neighbour.
    /// `Write` and `Experimental` share a rank because `Experimental` is a
    /// documented alias of `Write`, not an unmeasured placeholder.
    fn tier(self) -> u8 {
        match self {
            OperatingMode::ReadOnly => 0,
            OperatingMode::Manufacturing => 1,
            OperatingMode::Write | OperatingMode::Experimental => 2,
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
    fn manufacturing_restricts_write_and_experimental() {
        // Manufacturing sits strictly between ReadOnly and Write: it can
        // restrict-to from either of Write/Experimental (D69: never the
        // other way, see restrict_to_never_elevates).
        let guard = ModeGuard::new(OperatingMode::Write);
        guard.restrict_to(OperatingMode::Manufacturing);
        assert_eq!(guard.current(), OperatingMode::Manufacturing);

        let guard = ModeGuard::new(OperatingMode::Experimental);
        guard.restrict_to(OperatingMode::Manufacturing);
        assert_eq!(guard.current(), OperatingMode::Manufacturing);
    }

    #[test]
    fn write_and_experimental_are_a_no_op_on_each_other() {
        // Experimental is a deliberate alias of Write (same tier): neither
        // can restrict-to the other.
        let guard = ModeGuard::new(OperatingMode::Write);
        guard.restrict_to(OperatingMode::Experimental);
        assert_eq!(guard.current(), OperatingMode::Write);

        let guard = ModeGuard::new(OperatingMode::Experimental);
        guard.restrict_to(OperatingMode::Write);
        assert_eq!(guard.current(), OperatingMode::Experimental);
    }

    #[test]
    fn default_is_write_no_regression() {
        assert_eq!(ModeGuard::default().current(), OperatingMode::Write);
    }
}
