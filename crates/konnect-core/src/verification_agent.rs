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
                    return Ok(could_not_run(
                        task,
                        &document,
                        Some(check),
                        Some(revision),
                        &error.to_string(),
                    ))
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
        })
    }
}

fn could_not_run(
    task: TaskState,
    document: &Path,
    check: Option<Check>,
    revision: Option<String>,
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
        assert!(!outcome
            .task
            .verified_facts
            .iter()
            .any(|fact| fact.contains("PASS")));
    }

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
        assert!(outcome.task.verified_facts.is_empty());
    }
}
