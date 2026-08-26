//! Folding a DRC report into a readiness verdict.
//!
//! Two tools answer "is this board ready" — `validate_for_manufacturing` and
//! `run_design_review` — and neither of them ever ran DRC. Their only routing
//! test was `net_count > 3 && track_count == 0`, which fires on a board with
//! no copper at all and clears a board routed all but one net.
//!
//! The three passes of `kicad-cli pcb drc` answer that question properly, and
//! [`crate::tools::cli::DrcReport`] already distinguishes a pass that found
//! nothing (`Some(vec![])`) from a pass that never ran (`None`). This module
//! is the one place that turns either into words, so both verdicts say the
//! same thing and a later change to `validate_for_manufacturing` composes
//! with it instead of re-deriving it.
//!
//! The rule the whole module exists for: **evidence nobody gathered is never
//! a clean bill**. A DRC that could not run summarises as `null`, not as an
//! object of zeroes, and drives the verdict to `INCOMPLETE`.

use super::cli::{self, DrcCategory, DrcReport};
use serde_json::{json, Value};
use std::path::Path;

/// What a verdict knows about DRC.
pub(crate) enum DrcEvidence {
    /// `kicad-cli` ran; this is what it reported.
    Measured(DrcReport),
    /// It did not run. The string is why, verbatim, so the verdict can name
    /// the missing evidence rather than shrug.
    Unavailable(String),
}

/// One thing DRC has to say, in the shape both callers reduce to their own
/// finding type.
pub(crate) struct DrcFinding {
    pub severity: &'static str,
    pub issue: String,
    pub fix: String,
}

/// A DRC report, read as a readiness signal.
pub(crate) struct DrcGate {
    /// The counters, or `Value::Null` when nothing was measured. Never an
    /// object of zeroes standing in for an absent report.
    pub summary: Value,
    pub findings: Vec<DrcFinding>,
    /// The report is short at least one pass — the verdict cannot clear the
    /// board even if every finding it *does* have is benign.
    pub incomplete: bool,
    /// `unconnected_items` was actually measured. The only condition under
    /// which the old track-count heuristic has anything to add.
    pub connectivity_measured: bool,
}

/// Run DRC for a verdict, turning every failure mode — no configured binary,
/// a spawn error, an unreadable board — into [`DrcEvidence::Unavailable`]
/// rather than an error that would abort the whole review.
pub(crate) async fn gather(cli_path: &str, board: &Path, refill_zones: bool) -> DrcEvidence {
    if cli_path.trim().is_empty() {
        return DrcEvidence::Unavailable(
            "no kicad-cli path is configured for this server".to_string(),
        );
    }
    match cli::run_drc(cli_path, board, refill_zones).await {
        Ok(report) => DrcEvidence::Measured(report),
        Err(e) => DrcEvidence::Unavailable(format!("{e:#}")),
    }
}

fn category_words(category: DrcCategory) -> (&'static str, &'static str) {
    match category {
        DrcCategory::Violations => (
            "design-rule violation",
            "Open the board in pcbnew and clear the DRC panel before ordering",
        ),
        DrcCategory::UnconnectedItems => (
            "unconnected item",
            "Route the remaining connections (route_trace or autoroute), then re-run DRC",
        ),
        DrcCategory::SchematicParity => (
            "schematic-parity difference",
            "Re-sync the PCB from the schematic (Tools > Update PCB from Schematic)",
        ),
    }
}

fn examples(violations: &[&cli::DrcViolation]) -> String {
    let shown: Vec<&str> = violations
        .iter()
        .take(2)
        .map(|v| v.description.as_str())
        .filter(|d| !d.is_empty())
        .collect();
    if shown.is_empty() {
        String::new()
    } else {
        format!(" — e.g. {}", shown.join("; "))
    }
}

/// Read an evidence value as findings, counters and an incompleteness flag.
pub(crate) fn assess(evidence: &DrcEvidence) -> DrcGate {
    let report = match evidence {
        DrcEvidence::Unavailable(reason) => {
            return DrcGate {
                summary: Value::Null,
                findings: vec![DrcFinding {
                    severity: "warning",
                    issue: format!(
                        "DRC did not run, so no design rule on this board has been checked: {reason}"
                    ),
                    fix: "Install KiCAD 10 and point the server at its kicad-cli, then re-run: \
                          a verdict with no DRC behind it is not a clearance"
                        .to_string(),
                }],
                incomplete: true,
                connectivity_measured: false,
            };
        }
        DrcEvidence::Measured(report) => report,
    };

    let mut findings = Vec::new();
    for category in [
        DrcCategory::Violations,
        DrcCategory::UnconnectedItems,
        DrcCategory::SchematicParity,
    ] {
        let Some(list) = category_list(report, category) else {
            continue;
        };
        let (noun, fix) = category_words(category);
        for severity in ["error", "warning"] {
            let hits: Vec<&cli::DrcViolation> =
                list.iter().filter(|v| v.severity == severity).collect();
            if hits.is_empty() {
                continue;
            }
            findings.push(DrcFinding {
                severity,
                issue: format!(
                    "DRC: {} {}{} of severity {}{}",
                    hits.len(),
                    noun,
                    if hits.len() == 1 { "" } else { "s" },
                    severity,
                    examples(&hits)
                ),
                fix: fix.to_string(),
            });
        }
    }

    let missing: Vec<&'static str> = report
        .missing_categories()
        .iter()
        .map(|c| c.json_key())
        .collect();
    for key in &missing {
        findings.push(DrcFinding {
            severity: "warning",
            issue: format!(
                "DRC did not report a '{key}' section, so that pass is unmeasured — \
                 its absence is not zero findings"
            ),
            fix: "Re-run DRC with a kicad-cli that emits every section of \
                  schemas.kicad.org/drc.v1.json before treating the board as clear"
                .to_string(),
        });
    }

    DrcGate {
        summary: json!({
            "violations": report.violations.as_ref().map(Vec::len),
            "unconnected_items": report.unconnected_items.as_ref().map(Vec::len),
            "schematic_parity": report.schematic_parity.as_ref().map(Vec::len),
            "errors": report.error_count(),
            "missing_categories": missing,
        }),
        findings,
        incomplete: !missing.is_empty(),
        connectivity_measured: report.unconnected_items.is_some(),
    }
}

fn category_list(report: &DrcReport, category: DrcCategory) -> Option<&Vec<cli::DrcViolation>> {
    match category {
        DrcCategory::Violations => report.violations.as_ref(),
        DrcCategory::UnconnectedItems => report.unconnected_items.as_ref(),
        DrcCategory::SchematicParity => report.schematic_parity.as_ref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation(severity: &str, category: DrcCategory, description: &str) -> cli::DrcViolation {
        cli::DrcViolation {
            severity: severity.to_string(),
            description: description.to_string(),
            pos: None,
            rule: None,
            category,
            items: Vec::new(),
        }
    }

    /// The distinction P.6.1 built `Option` into the report for, carried all
    /// the way to the verdict: absent is not empty.
    #[test]
    fn a_measured_empty_pass_and_an_absent_pass_do_not_read_alike() {
        let clean = assess(&DrcEvidence::Measured(DrcReport {
            violations: Some(vec![]),
            unconnected_items: Some(vec![]),
            schematic_parity: Some(vec![]),
        }));
        assert!(!clean.incomplete);
        assert!(clean.findings.is_empty());
        assert_eq!(clean.summary["unconnected_items"], json!(0));

        let short = assess(&DrcEvidence::Measured(DrcReport {
            violations: Some(vec![]),
            unconnected_items: None,
            schematic_parity: Some(vec![]),
        }));
        assert!(short.incomplete);
        assert!(!short.connectivity_measured);
        assert!(short.summary["unconnected_items"].is_null());
    }

    #[test]
    fn each_category_is_counted_under_its_own_name() {
        let gate = assess(&DrcEvidence::Measured(DrcReport {
            violations: Some(vec![violation(
                "warning",
                DrcCategory::Violations,
                "silk over pad",
            )]),
            unconnected_items: Some(vec![violation(
                "error",
                DrcCategory::UnconnectedItems,
                "Missing connection: Pad 2 [SCL] on C1",
            )]),
            schematic_parity: Some(vec![]),
        }));
        assert_eq!(gate.summary["errors"], json!(1));
        assert_eq!(gate.findings.len(), 2);
        assert!(gate
            .findings
            .iter()
            .any(|f| f.severity == "error" && f.issue.contains("unconnected item")));
        assert!(gate
            .findings
            .iter()
            .any(|f| f.severity == "warning" && f.issue.contains("design-rule violation")));
    }
}
