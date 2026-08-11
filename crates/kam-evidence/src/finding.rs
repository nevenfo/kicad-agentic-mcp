//! Validator findings that keep their identity between runs.
//!
//! "ERC went from 4 errors to 2" is a weaker statement than it sounds: it is
//! also what happens when two are fixed and two others are introduced. The only
//! way to tell those apart is for a finding to be the *same* finding across
//! runs, and prose is not an identity — KiCAD rewords its descriptions between
//! versions, and two unconnected pins on one sheet share every word.
//!
//! So a finding is identified by a digest of what it is and where it is, and a
//! fix is proven by an id that disappeared rather than by a count that fell.
//! The id is computed in the constructor, so a caller cannot forget to set one.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Length of the hex id. 12 hex characters is 48 bits: collision-safe for the
/// thousands of findings one project produces, short enough to read in a diff.
const ID_HEX: usize = 12;

/// How much a finding matters, in the validator's own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocks the design.
    Error,
    /// Worth reading, does not block.
    Warning,
    /// Informational, excluded from both counts.
    Info,
}

impl Severity {
    /// Classify a validator's own severity string. Anything unrecognised is an
    /// error: a finding whose severity cannot be read must not be silently
    /// downgraded into something nobody looks at.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "warning" | "warn" => Self::Warning,
            "info" | "ignore" | "exclusion" | "excluded" => Self::Info,
            _ => Self::Error,
        }
    }
}

/// One thing a validator said about a design.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable across runs: a digest of validator, rule and location.
    pub id: String,
    /// Which validator produced it, e.g. `erc`.
    pub validator: String,
    /// How much it matters.
    pub severity: Severity,
    /// The rule that fired, in the validator's vocabulary.
    pub rule: String,
    /// Where it is, in whatever form the validator can address.
    pub location: String,
    /// The sentence a human reads. Deliberately *not* part of the id.
    pub message: String,
}

impl Finding {
    /// The id `validator`/`rule`/`location` hashes to.
    #[must_use]
    pub fn compute_id(validator: &str, rule: &str, location: &str) -> String {
        let mut hasher = Sha256::new();
        // Field separator that cannot appear in a hashed field's meaning, so
        // ("a", "bc") and ("ab", "c") are different findings.
        hasher.update(validator.as_bytes());
        hasher.update([0u8]);
        hasher.update(rule.as_bytes());
        hasher.update([0u8]);
        hasher.update(location.as_bytes());
        let digest = hasher.finalize();
        digest
            .iter()
            .take(ID_HEX.div_ceil(2))
            .fold(String::with_capacity(ID_HEX), |mut acc, b| {
                use std::fmt::Write;
                let _ = write!(acc, "{b:02x}");
                acc
            })
    }
}

/// Counts a reply can carry in one line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Findings at [`Severity::Error`].
    pub errors: usize,
    /// Findings at [`Severity::Warning`].
    pub warnings: usize,
}

/// Everything one validator said about one document, with unique ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingSet {
    /// In insertion order, which is the validator's own order.
    pub findings: Vec<Finding>,
}

impl FindingSet {
    /// An empty set — a document that passed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a finding, giving it an id that no other finding in this set has.
    ///
    /// Two genuinely identical findings — same rule, same location — are
    /// distinguished by an ordinal folded into the digest. Collapsing them into
    /// one would under-report, and giving them the same id would make one of
    /// them look fixed the moment the other is.
    pub fn push(
        &mut self,
        validator: &str,
        severity: Severity,
        rule: &str,
        location: &str,
        message: impl Into<String>,
    ) {
        let mut ordinal = 0usize;
        let mut key = location.to_string();
        let mut id = Finding::compute_id(validator, rule, &key);
        while self.findings.iter().any(|f| f.id == id) {
            ordinal += 1;
            key = format!("{location}#{ordinal}");
            id = Finding::compute_id(validator, rule, &key);
        }
        self.findings.push(Finding {
            id,
            validator: validator.to_string(),
            severity,
            rule: rule.to_string(),
            location: key,
            message: message.into(),
        });
    }

    /// Errors and warnings. Info findings are in neither count and still in the
    /// set, because the evidence pack should carry what the validator said.
    #[must_use]
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for finding in &self.findings {
            match finding.severity {
                Severity::Error => counts.errors += 1,
                Severity::Warning => counts.warnings += 1,
                Severity::Info => {}
            }
        }
        counts
    }

    /// How many findings there are, at any severity.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// Whether the validator found nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    fn ids(&self) -> BTreeSet<&str> {
        self.findings.iter().map(|f| f.id.as_str()).collect()
    }
}

/// What changed between two runs of the same validator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingDelta {
    /// Ids present before and gone now.
    pub fixed: Vec<String>,
    /// Ids that were not there before.
    pub introduced: Vec<String>,
    /// How many survived unchanged.
    pub unchanged: usize,
}

impl FindingDelta {
    /// Compare two runs by identity rather than by count.
    #[must_use]
    pub fn between(before: &FindingSet, after: &FindingSet) -> Self {
        let old = before.ids();
        let new = after.ids();
        Self {
            fixed: old.difference(&new).map(|s| (*s).to_string()).collect(),
            introduced: new.difference(&old).map(|s| (*s).to_string()).collect(),
            unchanged: old.intersection(&new).count(),
        }
    }

    /// Whether the two runs said exactly the same thing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fixed.is_empty() && self.introduced.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[(&str, Severity, &str, &str)]) -> FindingSet {
        let mut out = FindingSet::new();
        for (validator, severity, rule, location) in items {
            out.push(validator, *severity, rule, location, "some prose");
        }
        out
    }

    #[test]
    fn an_id_survives_a_reworded_message() {
        let mut a = FindingSet::new();
        a.push(
            "erc",
            Severity::Error,
            "pin_not_connected",
            "/:R1.1",
            "Pin not connected",
        );
        let mut b = FindingSet::new();
        b.push(
            "erc",
            Severity::Error,
            "pin_not_connected",
            "/:R1.1",
            "Pin is not connected",
        );
        assert_eq!(a.findings[0].id, b.findings[0].id);
    }

    #[test]
    fn a_different_location_is_a_different_finding() {
        let s = set(&[
            ("erc", Severity::Error, "pin_not_connected", "/:R1.1"),
            ("erc", Severity::Error, "pin_not_connected", "/:R2.1"),
        ]);
        assert_ne!(s.findings[0].id, s.findings[1].id);
    }

    #[test]
    fn two_identical_findings_keep_separate_ids() {
        let s = set(&[
            ("drc", Severity::Error, "clearance", "(10.0,20.0)"),
            ("drc", Severity::Error, "clearance", "(10.0,20.0)"),
        ]);
        assert_ne!(s.findings[0].id, s.findings[1].id);
        assert_eq!(s.counts().errors, 2);
    }

    #[test]
    fn field_boundaries_are_not_ambiguous() {
        assert_ne!(
            Finding::compute_id("erc", "a", "bc"),
            Finding::compute_id("erc", "ab", "c")
        );
    }

    #[test]
    fn a_fix_is_an_id_that_left_not_a_count_that_fell() {
        let before = set(&[
            ("erc", Severity::Error, "pin_not_connected", "/:R1.1"),
            ("erc", Severity::Error, "pin_not_connected", "/:R2.1"),
        ]);
        let after = set(&[
            ("erc", Severity::Error, "pin_not_connected", "/:R2.1"),
            ("erc", Severity::Error, "power_pin_not_driven", "/:U1.8"),
        ]);
        let delta = FindingDelta::between(&before, &after);
        // The counts are identical; the design is not.
        assert_eq!(before.counts(), after.counts());
        assert_eq!(delta.fixed.len(), 1);
        assert_eq!(delta.introduced.len(), 1);
        assert_eq!(delta.unchanged, 1);
        assert!(!delta.is_empty());
    }

    #[test]
    fn info_findings_are_kept_but_not_counted() {
        let s = set(&[
            ("erc", Severity::Info, "exclusion", "/:R1.1"),
            ("erc", Severity::Warning, "unannotated", "/:R2"),
        ]);
        assert_eq!(
            s.counts(),
            Counts {
                errors: 0,
                warnings: 1
            }
        );
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn an_unreadable_severity_is_an_error_not_an_omission() {
        assert_eq!(Severity::parse("Warning"), Severity::Warning);
        assert_eq!(Severity::parse("error"), Severity::Error);
        assert_eq!(Severity::parse("catastrophe"), Severity::Error);
        assert_eq!(Severity::parse("exclusion"), Severity::Info);
    }
}
