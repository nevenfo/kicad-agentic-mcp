//! Deterministic verification turns for the explicit Agent gateway.
//!
//! The only verdict source is `kicad-cli`, or its exact-revision cache entry.

use crate::evidence::validators::{self, Cache, Check};
use kam_evidence::EvidenceStore;
use kam_state::{DocState, TaskError, TaskState, TaskStore};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why a verdict is not `PASS` — the axis a recovery loop actually branches
/// on, distinct from the verdict itself.
///
/// `design` and `environment` drive opposite agent loops (plan.md D.6.3): a
/// `design` failure sends the agent back to the schematic or board, an
/// `environment` failure sends it to fix `kicad-cli` or launch KiCAD, and
/// neither loop helps the other's problem. Conflating them is the mistake
/// this type exists to make impossible — see [`could_not_run`], which cannot
/// construct [`FailureMode::Design`] at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    /// The validator ran, on the right revision, and found real findings.
    /// Only reachable from a completed run with `errors > 0` — never from a
    /// `COULD_NOT_RUN` verdict (INV1: a validator that could not run is a
    /// failure, never zero findings, and never evidence the design is bad).
    Design,
    /// The tool the validator needs is missing or refuses to start
    /// (`kicad-cli` absent, KiCAD not running).
    Environment,
    /// Everything needed is present but misconfigured — wrong path, or a
    /// write refused by the current `KONNECT_MODE`
    /// ([`crate::mcp::error::ToolErrorKind::WriteRefusedByMode`]).
    Configuration,
    /// Nothing here can decide automatically; a human has to look.
    ManualReview,
}

/// Failure modes a validator that never ran is allowed to report.
///
/// `Design` has no variant here — a caller that never obtained findings
/// cannot name it, so [`could_not_run`] cannot produce
/// [`FailureMode::Design`] regardless of what reason string it is given.
/// This is the type-level form of INV1's rule for this struct.
///
/// No `ManualReview` variant either, deliberately: nothing in `verify()`
/// today has grounds to say a human, rather than a fixable environment or
/// configuration problem, is what stands between the caller and a verdict —
/// [`FailureMode::ManualReview`] stays reserved on the public type for a
/// site (e.g. a GUI-only capability handler) that does.
#[derive(Debug, Clone, Copy)]
enum CouldNotRunMode {
    Environment,
    Configuration,
}

impl From<CouldNotRunMode> for FailureMode {
    fn from(mode: CouldNotRunMode) -> Self {
        match mode {
            CouldNotRunMode::Environment => FailureMode::Environment,
            CouldNotRunMode::Configuration => FailureMode::Configuration,
        }
    }
}

/// Structured deterministic verification result.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationOutcome {
    /// Durable task after a completed validator verdict was recorded.
    pub task: TaskState,
    /// Checked document.
    pub document: String,
    /// `erc` or `drc` for supported documents.
    pub check: Option<&'static str>,
    /// Content revision described by the verdict.
    pub revision: Option<String>,
    /// `PASS`, `FAIL`, or `COULD_NOT_RUN`.
    pub verdict: &'static str,
    /// `kicad-cli`, `validator_cache`, or `none`.
    pub source: &'static str,
    /// Error findings after a completed validator run.
    pub errors: Option<usize>,
    /// Warning findings after a completed validator run.
    pub warnings: Option<usize>,
    /// Structured evidence handle after a completed validator run.
    pub evidence: Option<String>,
    /// Reason execution could not complete.
    pub reason: Option<String>,
    /// Why the verdict is not `PASS`. Present exactly when `verdict` is not
    /// `"PASS"`, absent when it is (D.6.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_mode: Option<FailureMode>,
}

/// Verifies through existing validator, cache and evidence components.
pub struct VerificationAgent {
    tasks: Arc<TaskStore>,
    cache: Arc<Cache>,
    evidence: Arc<EvidenceStore>,
    kicad_cli: String,
}

impl VerificationAgent {
    /// Builds an agent sharing the supervisor task store.
    #[must_use]
    pub fn new(
        tasks: Arc<TaskStore>,
        cache: Arc<Cache>,
        evidence: Arc<EvidenceStore>,
        kicad_cli: impl Into<String>,
    ) -> Self {
        Self {
            tasks,
            cache,
            evidence,
            kicad_cli: kicad_cli.into(),
        }
    }

    /// Runs or retrieves the exact-revision `kicad-cli` verdict.
    pub async fn verify(
        &self,
        task_id: &str,
        document: impl Into<PathBuf>,
    ) -> Result<VerificationOutcome, TaskError> {
        let document = document.into();
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| TaskError::Unknown(task_id.to_string()))?;
        let Some(check) = Check::of_path(&document) else {
            return Ok(could_not_run(
                task,
                &document,
                None,
                None,
                CouldNotRunMode::Configuration,
                "unsupported document type",
            ));
        };
        let state = match DocState::read(&document) {
            Ok(state) => state,
            Err(error) => {
                return Ok(could_not_run(
                    task,
                    &document,
                    Some(check),
                    None,
                    CouldNotRunMode::Configuration,
                    &error.to_string(),
                ))
            }
        };
        let revision = state.token();
        if matches!(state, DocState::Absent) {
            return Ok(could_not_run(
                task,
                &document,
                Some(check),
                Some(revision),
                CouldNotRunMode::Configuration,
                "document is absent",
            ));
        }
        let (findings, source) = match self.cache.get(&document, &revision) {
            Some(findings) => (findings, "validator_cache"),
            None => match validators::run(check, &self.kicad_cli, &document).await {
                Ok(findings) => {
                    self.cache.put(&document, &revision, findings.clone());
                    (findings, "kicad-cli")
                }
                Err(error) => {
                    // `kicad-cli` failing to launch or exiting abnormally
                    // is the tool being missing or refusing to run, not a
                    // finding about the design — INV1 requires this be a
                    // failed postcondition, never zero findings.
                    return Ok(could_not_run(
                        task,
                        &document,
                        Some(check),
                        Some(revision),
                        CouldNotRunMode::Environment,
                        &error.to_string(),
                    ));
                }
            },
        };
        let counts = findings.counts();
        let verdict = if counts.errors == 0 { "PASS" } else { "FAIL" };
        let handle = self.evidence.put(
            "verification",
            format!(
                "{} {}: {}E {}W",
                check.name(),
                document.display(),
                counts.errors,
                counts.warnings
            ),
            json!({
                "source": source, "check": check.name(), "document": document, "revision": revision,
                "errors": counts.errors, "warnings": counts.warnings, "findings": findings.findings,
            }),
        );
        let evidence = handle.uri;
        let fact = format!(
            "{} {} {} at {}: {} errors, {} warnings ({source})",
            check.name(),
            document.display(),
            verdict,
            revision,
            counts.errors,
            counts.warnings
        );
        let (task, ()) = self.tasks.update(task_id, |state| {
            state.note_revision(document.display().to_string(), revision.clone());
            state.attach_evidence(evidence.clone());
            state.add_verified_fact(fact);
            Ok(())
        })?;
        Ok(VerificationOutcome {
            task,
            document: document.display().to_string(),
            check: Some(check.name()),
            revision: Some(revision),
            verdict,
            source,
            errors: Some(counts.errors),
            warnings: Some(counts.warnings),
            evidence: Some(evidence),
            reason: None,
            // `Design` is only ever assigned here, from a run that actually
            // produced `counts` — never from `could_not_run`.
            failure_mode: (counts.errors > 0).then_some(FailureMode::Design),
        })
    }
}

/// Builds a `COULD_NOT_RUN` outcome. `mode` is a [`CouldNotRunMode`], not a
/// [`FailureMode`] — the type itself is why this function can never report
/// `FailureMode::Design` (D.6.3 / INV1).
fn could_not_run(
    task: TaskState,
    document: &Path,
    check: Option<Check>,
    revision: Option<String>,
    mode: CouldNotRunMode,
    reason: &str,
) -> VerificationOutcome {
    VerificationOutcome {
        task,
        document: document.display().to_string(),
        check: check.map(Check::name),
        revision,
        verdict: "COULD_NOT_RUN",
        source: "none",
        errors: None,
        warnings: None,
        evidence: None,
        reason: Some(reason.to_string()),
        failure_mode: Some(mode.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_evidence::{FindingSet, Severity};

    fn setup() -> (tempfile::TempDir, PathBuf, Arc<TaskStore>, String) {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("board.kicad_sch");
        std::fs::write(&document, "(kicad_sch)").unwrap();
        let tasks = Arc::new(TaskStore::default());
        let task = tasks.start("verify board");
        (directory, document, tasks, task.id)
    }

    #[tokio::test]
    async fn cached_validator_verdict_is_the_only_verified_fact_source() {
        let (_directory, document, tasks, task_id) = setup();
        let cache = Arc::new(Cache::default());
        cache.put(
            &document,
            &DocState::read(&document).unwrap().token(),
            FindingSet::new(),
        );
        let outcome = VerificationAgent::new(tasks, cache, Arc::default(), "must-not-run")
            .verify(&task_id, &document)
            .await
            .unwrap();
        assert_eq!(outcome.verdict, "PASS");
        assert_eq!(outcome.source, "validator_cache");
        assert!(outcome.evidence.is_some());
        assert_eq!(outcome.failure_mode, None, "PASS carries no failure_mode");
        assert!(outcome
            .task
            .verified_facts
            .iter()
            .any(|fact| fact.contains("PASS")));
    }

    #[tokio::test]
    async fn failed_validator_is_recorded_as_fail_not_pass() {
        let (_directory, document, tasks, task_id) = setup();
        let cache = Arc::new(Cache::default());
        let mut findings = FindingSet::new();
        findings.push("erc", Severity::Error, "rule", "/", "deterministic failure");
        cache.put(
            &document,
            &DocState::read(&document).unwrap().token(),
            findings,
        );
        let outcome = VerificationAgent::new(tasks, cache, Arc::default(), "must-not-run")
            .verify(&task_id, &document)
            .await
            .unwrap();
        assert_eq!(outcome.verdict, "FAIL");
        assert_eq!(outcome.errors, Some(1));
        assert_eq!(
            outcome.failure_mode,
            Some(FailureMode::Design),
            "real findings from a completed run must be design, not environment/manual"
        );
        assert!(!outcome
            .task
            .verified_facts
            .iter()
            .any(|fact| fact.contains("PASS")));
    }

    /// Failure injection: the validator binary does not exist, so it never
    /// ran and never produced a finding. D.6.3 / INV1: this must read
    /// `environment`, never `design` — a broken environment and a broken
    /// design have to drive opposite agent loops.
    #[tokio::test]
    async fn unavailable_validator_is_never_a_verified_pass() {
        let (_directory, document, tasks, task_id) = setup();
        let outcome = VerificationAgent::new(
            tasks,
            Arc::default(),
            Arc::default(),
            "definitely-not-kicad-cli",
        )
        .verify(&task_id, &document)
        .await
        .unwrap();
        assert_eq!(outcome.verdict, "COULD_NOT_RUN");
        assert!(outcome.reason.is_some());
        assert_eq!(outcome.failure_mode, Some(FailureMode::Environment));
        assert!(outcome.task.verified_facts.is_empty());
    }

    /// Failure injection: an unsupported document type never reaches a
    /// validator either — same INV1 guarantee, different `CouldNotRunMode`.
    #[tokio::test]
    async fn unsupported_document_could_not_run_is_configuration_not_design() {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("board.txt");
        std::fs::write(&document, "not a kicad document").unwrap();
        let tasks = Arc::new(TaskStore::default());
        let task = tasks.start("verify board");
        let outcome = VerificationAgent::new(tasks, Arc::default(), Arc::default(), "unused")
            .verify(&task.id, &document)
            .await
            .unwrap();
        assert_eq!(outcome.verdict, "COULD_NOT_RUN");
        assert_eq!(outcome.failure_mode, Some(FailureMode::Configuration));
    }

    /// The structural guarantee, proven directly rather than trusted from
    /// review: every [`CouldNotRunMode`] converts to a [`FailureMode`] other
    /// than `Design`. A new variant added to `CouldNotRunMode` that somehow
    /// mapped to `Design` would fail this immediately.
    #[test]
    fn could_not_run_mode_never_converts_to_design() {
        for mode in [CouldNotRunMode::Environment, CouldNotRunMode::Configuration] {
            assert_ne!(FailureMode::from(mode), FailureMode::Design);
        }
    }
}
