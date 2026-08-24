//! Asking KiCAD whether the design still holds.
//!
//! The semantic diff says what moved. It cannot say whether the board still
//! passes, and the project's own history says why that distinction matters:
//! Konnect's internal connectivity analysis once reported zero single-pin nets
//! on a schematic where `kicad-cli sch erc` reported six unconnected pins
//! (`progress.md`, E7). An internal check is not a validator, and a batch that
//! claimed otherwise would be exactly the self-reported proof this design
//! exists to avoid.
//!
//! So the verdict comes from `kicad-cli`, and it comes as findings with stable
//! ids, so "ERC 4 → 0" can be backed by the four ids that disappeared.
//!
//! ## Why the baseline is cached rather than recomputed
//!
//! A before/after pair would mean running the validator twice per batch, and
//! ERC on a real project is seconds, not milliseconds. Instead each verdict is
//! remembered against the content revision it was computed for, so the *next*
//! batch on the same document finds its own baseline already there. The first
//! verification of a session has no baseline and says so — `baseline:
//! "unknown"` — rather than implying the design just went from zero to zero.

use kam_evidence::{FindingSet, Severity};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Which of KiCAD's validators applies to a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// `kicad-cli sch erc`
    Erc,
    /// `kicad-cli pcb drc`
    Drc,
}

impl Check {
    /// The validator for a path, or `None` for a document neither checks.
    #[must_use]
    pub fn of_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("kicad_sch") => Some(Self::Erc),
            Some("kicad_pcb") => Some(Self::Drc),
            _ => None,
        }
    }

    /// Short name, as it appears in a reply and in a finding's `validator`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Erc => "erc",
            Self::Drc => "drc",
        }
    }
}

/// A postcondition name a plan may declare in `validators`.
///
/// A plan is `kam-plan`'s concern only up to the wire name — recognising it as
/// `erc` vs `erc_clean` and knowing which [`Check`] and threshold each implies
/// is KiCAD knowledge, so it lives here rather than in `kam-plan` (D11: that
/// crate stays ignorant of KiCAD to keep its narrower license honest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Postcondition {
    /// The plan must introduce no new ERC finding (delta vs. the baseline).
    Erc,
    /// The plan must leave zero ERC errors, absolutely.
    ErcClean,
    /// The plan must introduce no new DRC finding (delta vs. the baseline).
    Drc,
    /// The plan must leave zero DRC errors, absolutely.
    DrcClean,
}

impl Postcondition {
    /// Parse a wire name, or `None` for anything else — a plan is refused
    /// rather than silently ignoring a misspelled postcondition (D17: a
    /// validator that does not run in silence is worse than an error).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "erc" => Some(Self::Erc),
            "erc_clean" => Some(Self::ErcClean),
            "drc" => Some(Self::Drc),
            "drc_clean" => Some(Self::DrcClean),
            _ => None,
        }
    }

    /// The wire name, for error messages and reports.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Erc => "erc",
            Self::ErcClean => "erc_clean",
            Self::Drc => "drc",
            Self::DrcClean => "drc_clean",
        }
    }

    /// Which validator applies.
    #[must_use]
    pub fn check(self) -> Check {
        match self {
            Self::Erc | Self::ErcClean => Check::Erc,
            Self::Drc | Self::DrcClean => Check::Drc,
        }
    }

    /// Whether this is the absolute-zero-errors form, which needs no baseline.
    #[must_use]
    pub fn is_clean(self) -> bool {
        matches!(self, Self::ErcClean | Self::DrcClean)
    }
}

/// Run the validator and translate its violations into findings.
///
/// # Errors
///
/// Whatever `kicad-cli` failed with. A validator that could not run must
/// surface as an error and never as zero findings: a missing `kicad-cli` once
/// made an empty schematic look like a passing one (`progress.md`, E4).
pub async fn run(check: Check, cli: &str, path: &Path) -> anyhow::Result<FindingSet> {
    let mut set = FindingSet::new();
    match check {
        Check::Erc => {
            for v in crate::tools::cli::run_erc(cli, path).await? {
                let sheet = v.sheet.as_deref().unwrap_or("/");
                let location = match &v.pos {
                    Some(p) => format!("{sheet}@{:.3},{:.3}", p.x, p.y),
                    None => sheet.to_string(),
                };
                set.push(
                    check.name(),
                    Severity::parse(&v.severity),
                    v.rule.as_deref().unwrap_or(UNSPECIFIED_RULE),
                    &location,
                    v.description,
                );
            }
        }
        Check::Drc => {
            // Zones are not refilled: refilling rewrites the board, and a
            // verification step that mutates the document it is verifying
            // would invalidate the revision its own verdict is cached under.
            let report = crate::tools::cli::run_drc(cli, path, false).await?;
            let missing = report.missing_categories();
            if !missing.is_empty() {
                // A report missing a category ran an incomplete check, not a
                // clean one — surfacing it as zero findings for that category
                // is exactly the false-clean verdict this gate exists to
                // rule out (see this file's module doc, and progress.md E4).
                let names: Vec<&str> = missing.iter().map(|c| c.json_key()).collect();
                anyhow::bail!(
                    "DRC report is missing {}: kicad-cli did not run the full check",
                    names.join(", ")
                );
            }
            for v in report.all() {
                let location = match &v.pos {
                    Some(p) => format!("@{:.3},{:.3}", p.x, p.y),
                    None => String::from("@?"),
                };
                set.push(
                    check.name(),
                    Severity::parse(&v.severity),
                    v.rule.as_deref().unwrap_or(UNSPECIFIED_RULE),
                    &location,
                    v.description.clone(),
                );
            }
        }
    }
    Ok(set)
}

/// Rule name for a violation KiCAD did not classify. Kept explicit so a
/// finding id built from it is still stable, rather than hashing an empty
/// string that could mean anything.
const UNSPECIFIED_RULE: &str = "unspecified";

/// How many (document, revision) verdicts are remembered.
const CACHE_CAPACITY: usize = 64;

/// Verdicts remembered against the content revision they describe.
///
/// Keyed by revision, not by path alone: a document that changed has no valid
/// cached verdict, which is the property that makes the cache safe to consult
/// without re-reading the file.
#[derive(Debug, Default)]
pub struct Cache {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<(PathBuf, String), FindingSet>,
    order: VecDeque<(PathBuf, String)>,
}

impl Cache {
    /// The verdict for this document at this revision, if one was computed.
    #[must_use]
    pub fn get(&self, path: &Path, revision: &str) -> Option<FindingSet> {
        let inner = self.lock();
        inner
            .entries
            .get(&(path.to_path_buf(), revision.to_string()))
            .cloned()
    }

    /// Remember a verdict.
    pub fn put(&self, path: &Path, revision: &str, set: FindingSet) {
        let key = (path.to_path_buf(), revision.to_string());
        let mut inner = self.lock();
        if inner.entries.insert(key.clone(), set).is_none() {
            inner.order.push_back(key);
        }
        while inner.order.len() > CACHE_CAPACITY {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            inner.entries.remove(&oldest);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_documents_kicad_can_check_have_a_validator() {
        assert_eq!(Check::of_path(Path::new("a.kicad_sch")), Some(Check::Erc));
        assert_eq!(Check::of_path(Path::new("a.kicad_pcb")), Some(Check::Drc));
        assert_eq!(Check::of_path(Path::new("a.kicad_pro")), None);
    }

    #[test]
    fn only_the_four_recognised_names_parse() {
        assert_eq!(Postcondition::parse("erc"), Some(Postcondition::Erc));
        assert_eq!(
            Postcondition::parse("erc_clean"),
            Some(Postcondition::ErcClean)
        );
        assert_eq!(Postcondition::parse("drc"), Some(Postcondition::Drc));
        assert_eq!(
            Postcondition::parse("drc_clean"),
            Some(Postcondition::DrcClean)
        );
        assert_eq!(Postcondition::parse("ERC"), None);
        assert_eq!(Postcondition::parse("erc-clean"), None);
        assert_eq!(Postcondition::parse(""), None);
    }

    #[test]
    fn clean_variants_share_the_check_with_their_delta_counterpart() {
        assert_eq!(Postcondition::Erc.check(), Postcondition::ErcClean.check());
        assert_eq!(Postcondition::Drc.check(), Postcondition::DrcClean.check());
        assert!(Postcondition::ErcClean.is_clean() && !Postcondition::Erc.is_clean());
        assert!(Postcondition::DrcClean.is_clean() && !Postcondition::Drc.is_clean());
    }

    #[test]
    fn a_verdict_belongs_to_one_revision_only() {
        let cache = Cache::default();
        let path = Path::new("/p/a.kicad_sch");
        cache.put(path, "rev-1", FindingSet::new());
        assert!(cache.get(path, "rev-1").is_some());
        // The document moved on: the old verdict must not answer for it.
        assert!(cache.get(path, "rev-2").is_none());
    }

    #[test]
    fn the_cache_is_bounded() {
        let cache = Cache::default();
        let first = PathBuf::from("/p/0.kicad_sch");
        for i in 0..(CACHE_CAPACITY + 1) {
            cache.put(
                &PathBuf::from(format!("/p/{i}.kicad_sch")),
                "r",
                FindingSet::new(),
            );
        }
        assert!(cache.get(&first, "r").is_none());
    }
}
