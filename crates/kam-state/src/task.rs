//! What the work is, kept outside the model that is doing it.
//!
//! A long agent trajectory loses its objective in two different ways. The
//! obvious one is a context window that drops the first message. The subtler
//! one is behavioural: the objective is still in the window and the model
//! stops acting on it, because it is buried under twenty tool results. Both
//! have the same fix — the objective must not live in the transcript at all.
//!
//! So a task is a record: objective, constraints, what has been verified, what
//! failed and must not be retried, what evidence backs it. The model can be
//! compacted, restarted, or swapped for a different model, and the task is
//! still exactly what it was.
//!
//! Two properties make this more than a notepad:
//!
//! * **The anchor is rendered, never remembered.** [`TaskState::anchor`]
//!   produces a short block from the record itself, so a reminder cannot drift
//!   from the thing it is reminding about. A model that paraphrases its own
//!   objective back into its own prompt is the failure this prevents.
//! * **Hard constraints are refused rather than evicted.** Every other list
//!   drops its oldest entry when full. Constraints and success criteria do not:
//!   silently forgetting "do not touch the RF section" is the one bound this
//!   module must not enforce quietly.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

/// Entries kept per list. Beyond this a task is not a task any more, it is a
/// project, and the answer is to split it rather than to grow the record.
pub const MAX_LIST: usize = 12;

/// Characters of objective the anchor carries. The anchor is re-sent
/// constantly, so it is budgeted in tokens rather than in completeness; the
/// full objective is one `get` away.
const ANCHOR_OBJECTIVE_CHARS: usize = 110;

/// Constraints named in the anchor. The rest are in the record.
const ANCHOR_CONSTRAINTS: usize = 3;

/// Where a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Being worked on.
    Active,
    /// Cannot proceed without something that is not here.
    Blocked,
    /// Finished by the agent, and of a kind that must not ship unreviewed.
    NeedsReview,
    /// Finished and verified.
    Done,
    /// Given up on, deliberately.
    Abandoned,
}

impl TaskStatus {
    /// Whether more work is expected on this task.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(self, Self::Active | Self::Blocked)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::NeedsReview => "needs_review",
            Self::Done => "done",
            Self::Abandoned => "abandoned",
        }
    }
}

/// Something that was tried and did not work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedAttempt {
    /// What was tried.
    pub what: String,
    /// Why it failed, in the failure's own words where possible.
    pub why: String,
}

/// How far along the task is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Sub-goals completed.
    pub done: usize,
    /// Sub-goals expected. Zero means "not planned in steps".
    pub total: usize,
}

/// Why a task record refused an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    /// No task with that id.
    Unknown(String),
    /// A bounded list that must not silently drop entries is full.
    ListFull {
        /// Which list.
        field: &'static str,
        /// The bound it hit.
        max: usize,
    },
}

impl TaskError {
    /// Stable code for the error catalogue.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unknown(_) => "unknown_task",
            Self::ListFull { .. } => "task_list_full",
        }
    }
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "No task '{id}'."),
            Self::ListFull { field, max } => write!(
                f,
                "'{field}' already holds {max} entries and must not drop one silently. \
                 Split the task, or remove an entry that no longer applies."
            ),
        }
    }
}

impl std::error::Error for TaskError {}

/// Everything known about one unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    /// Stable id, e.g. `task_3`.
    pub id: String,
    /// What success would be, in the caller's own words.
    pub objective: String,
    /// The part being worked on right now.
    pub subgoal: Option<String>,
    /// Must-hold conditions. Refused rather than evicted when full.
    pub constraints: Vec<String>,
    /// How completion will be judged. Refused rather than evicted when full.
    pub success_criteria: Vec<String>,
    /// Things established by a validator or a real read, not by inference.
    pub verified_facts: Vec<String>,
    /// Things taken on faith, which a reviewer must be able to see.
    pub assumptions: Vec<String>,
    /// Known unknowns.
    pub unresolved: Vec<String>,
    /// What was tried and failed, newest last.
    pub failed_attempts: Vec<FailedAttempt>,
    /// Approaches that must not be tried again.
    pub forbidden_repeats: Vec<String>,
    /// Document path to the revision this task last saw it at.
    pub revisions: BTreeMap<String, String>,
    /// Evidence handles produced while working on this task.
    pub evidence: Vec<String>,
    /// Sub-goal counter.
    pub progress: Progress,
    /// Batches applied under this task.
    pub batches: usize,
    /// Where the task is.
    pub status: TaskStatus,
}

impl TaskState {
    /// A fresh active task.
    #[must_use]
    pub fn new(id: impl Into<String>, objective: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            objective: objective.into(),
            subgoal: None,
            constraints: Vec::new(),
            success_criteria: Vec::new(),
            verified_facts: Vec::new(),
            assumptions: Vec::new(),
            unresolved: Vec::new(),
            failed_attempts: Vec::new(),
            forbidden_repeats: Vec::new(),
            revisions: BTreeMap::new(),
            evidence: Vec::new(),
            progress: Progress::default(),
            batches: 0,
            status: TaskStatus::Active,
        }
    }

    /// Add a hard constraint.
    ///
    /// # Errors
    ///
    /// [`TaskError::ListFull`] once [`MAX_LIST`] constraints are held. A hard
    /// constraint that fell off the end of a ring buffer is worse than one that
    /// was never accepted, because only the second is visible.
    pub fn add_constraint(&mut self, value: impl Into<String>) -> Result<(), TaskError> {
        push_strict(&mut self.constraints, "constraints", value.into())
    }

    /// Add a success criterion.
    ///
    /// # Errors
    ///
    /// [`TaskError::ListFull`], for the same reason as [`Self::add_constraint`].
    pub fn add_success_criterion(&mut self, value: impl Into<String>) -> Result<(), TaskError> {
        push_strict(&mut self.success_criteria, "success_criteria", value.into())
    }

    /// Record something a validator or a real read established.
    pub fn add_verified_fact(&mut self, value: impl Into<String>) {
        push_rolling(&mut self.verified_facts, value.into());
    }

    /// Record something taken on faith.
    pub fn add_assumption(&mut self, value: impl Into<String>) {
        push_rolling(&mut self.assumptions, value.into());
    }

    /// Record a known unknown.
    pub fn add_unresolved(&mut self, value: impl Into<String>) {
        push_rolling(&mut self.unresolved, value.into());
    }

    /// Record a failure, so the same wall is not walked into twice.
    pub fn add_failed_attempt(&mut self, what: impl Into<String>, why: impl Into<String>) {
        let attempt = FailedAttempt {
            what: what.into(),
            why: why.into(),
        };
        if self.failed_attempts.contains(&attempt) {
            return;
        }
        if self.failed_attempts.len() >= MAX_LIST {
            self.failed_attempts.remove(0);
        }
        self.failed_attempts.push(attempt);
    }

    /// Record an approach that must not be retried.
    pub fn forbid(&mut self, value: impl Into<String>) {
        push_rolling(&mut self.forbidden_repeats, value.into());
    }

    /// Note the revision a document is at, replacing any earlier note.
    pub fn note_revision(&mut self, path: impl Into<String>, revision: impl Into<String>) {
        self.revisions.insert(path.into(), revision.into());
    }

    /// Attach an evidence handle.
    pub fn attach_evidence(&mut self, handle: impl Into<String>) {
        let handle = handle.into();
        if self.evidence.contains(&handle) {
            return;
        }
        push_rolling(&mut self.evidence, handle);
    }

    /// The short block that goes at the end of a prompt.
    ///
    /// Rendered from the record every time, never stored: a reminder that can
    /// drift from what it reminds about is worse than no reminder. Budgeted at
    /// roughly 30–80 tokens, which is why it truncates rather than summarising
    /// — a summary would be a second thing that can be wrong.
    #[must_use]
    pub fn anchor(&self) -> String {
        let mut out = format!("ACTIVE TASK {}", self.id);
        if self.progress.total > 0 {
            out.push_str(&format!(
                " | {}/{}",
                self.progress.done, self.progress.total
            ));
        }
        if self.status != TaskStatus::Active {
            out.push_str(&format!(" | {}", self.status.as_str()));
        }
        out.push_str("\nGoal: ");
        out.push_str(&truncate(&self.objective, ANCHOR_OBJECTIVE_CHARS));
        if let Some(subgoal) = &self.subgoal {
            out.push_str("\nNow: ");
            out.push_str(&truncate(subgoal, ANCHOR_OBJECTIVE_CHARS));
        }
        if !self.constraints.is_empty() {
            out.push_str("\nMust: ");
            out.push_str(
                &self
                    .constraints
                    .iter()
                    .take(ANCHOR_CONSTRAINTS)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            if self.constraints.len() > ANCHOR_CONSTRAINTS {
                out.push_str(&format!(
                    " | +{} more",
                    self.constraints.len() - ANCHOR_CONSTRAINTS
                ));
            }
        }
        // Only the most recent, and only one: the anchor exists to stop a
        // repeat that is about to happen, not to hold a history.
        if let Some(last) = self.forbidden_repeats.last() {
            out.push_str("\nAvoid: ");
            out.push_str(&truncate(last, ANCHOR_OBJECTIVE_CHARS));
        }
        out
    }
}

/// A task as it appears in a listing: enough to choose one, not enough to work
/// from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSummary {
    /// Stable id.
    pub id: String,
    /// Truncated objective.
    pub objective: String,
    /// Where it is.
    pub status: TaskStatus,
    /// Sub-goal counter.
    pub progress: Progress,
}

fn push_strict(
    list: &mut Vec<String>,
    field: &'static str,
    value: String,
) -> Result<(), TaskError> {
    if list.contains(&value) {
        return Ok(());
    }
    if list.len() >= MAX_LIST {
        return Err(TaskError::ListFull {
            field,
            max: MAX_LIST,
        });
    }
    list.push(value);
    Ok(())
}

fn push_rolling(list: &mut Vec<String>, value: String) {
    if list.contains(&value) {
        return;
    }
    if list.len() >= MAX_LIST {
        list.remove(0);
    }
    list.push(value);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

/// How many tasks are remembered.
const STORE_CAPACITY: usize = 32;

/// The tasks this process knows about.
///
/// In memory and bounded, like the idempotency ledger and for the same reason:
/// durability across a restart would need a journal, and what this protects —
/// an objective surviving a compaction, a model swap or forty tool calls — is
/// measured in one session.
#[derive(Debug, Default)]
pub struct TaskStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    tasks: HashMap<String, TaskState>,
    order: VecDeque<String>,
    next_id: u64,
}

impl TaskStore {
    /// Open a task and return it.
    pub fn start(&self, objective: &str) -> TaskState {
        let mut inner = self.lock();
        inner.next_id += 1;
        let id = format!("task_{}", inner.next_id);
        let task = TaskState::new(&id, objective);
        inner.tasks.insert(id.clone(), task.clone());
        inner.order.push_back(id);
        while inner.order.len() > STORE_CAPACITY {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            inner.tasks.remove(&oldest);
        }
        task
    }

    /// One task, if it is still held.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<TaskState> {
        self.lock().tasks.get(id).cloned()
    }

    /// Every held task, oldest first.
    #[must_use]
    pub fn list(&self) -> Vec<TaskSummary> {
        let inner = self.lock();
        inner
            .order
            .iter()
            .filter_map(|id| inner.tasks.get(id))
            .map(|t| TaskSummary {
                id: t.id.clone(),
                objective: truncate(&t.objective, ANCHOR_OBJECTIVE_CHARS),
                status: t.status,
                progress: t.progress,
            })
            .collect()
    }

    /// Mutate a task in place and return it as it now is.
    ///
    /// # Errors
    ///
    /// [`TaskError::Unknown`] if the id is not held, or whatever `f` returned.
    /// A failed `f` still leaves its partial edits applied — the caller is
    /// mutating one record, not composing a transaction — so `f` should apply
    /// the fallible parts first.
    pub fn update<F, T>(&self, id: &str, f: F) -> Result<(TaskState, T), TaskError>
    where
        F: FnOnce(&mut TaskState) -> Result<T, TaskError>,
    {
        let mut inner = self.lock();
        let task = inner
            .tasks
            .get_mut(id)
            .ok_or_else(|| TaskError::Unknown(id.to_string()))?;
        let out = f(task)?;
        Ok((task.clone(), out))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchor_stays_short_and_names_what_matters() {
        let mut task = TaskState::new("task_1", "Add the 3V3 rail for the STM32 and decouple it");
        task.subgoal = Some("decoupling".to_string());
        task.add_constraint("do not touch the RF section").unwrap();
        task.add_constraint("ERC must stay at 0").unwrap();
        task.progress = Progress { done: 3, total: 5 };
        task.forbid("placing C7 at 100,80 — collides with U1");

        let anchor = task.anchor();
        assert!(anchor.starts_with("ACTIVE TASK task_1 | 3/5"), "{anchor}");
        assert!(anchor.contains("do not touch the RF section"), "{anchor}");
        assert!(anchor.contains("Avoid: placing C7"), "{anchor}");
        // ~4 characters per token: the budget is 30–80 tokens.
        assert!(
            anchor.len() < 320,
            "anchor is {} chars:\n{anchor}",
            anchor.len()
        );
    }

    #[test]
    fn the_anchor_is_rendered_from_the_record_not_from_a_copy() {
        let mut task = TaskState::new("task_1", "first objective");
        let before = task.anchor();
        task.objective = "second objective".to_string();
        assert_ne!(before, task.anchor());
        assert!(task.anchor().contains("second objective"));
    }

    #[test]
    fn a_hard_constraint_is_refused_rather_than_dropped() {
        let mut task = TaskState::new("task_1", "o");
        for i in 0..MAX_LIST {
            task.add_constraint(format!("c{i}")).unwrap();
        }
        let err = task.add_constraint("the one that matters").unwrap_err();
        assert_eq!(err.code(), "task_list_full");
        // The refusal is the point: nothing was silently forgotten.
        assert_eq!(task.constraints.len(), MAX_LIST);
        assert!(!task
            .constraints
            .contains(&"the one that matters".to_string()));
    }

    #[test]
    fn softer_lists_keep_the_newest_instead_of_refusing() {
        let mut task = TaskState::new("task_1", "o");
        for i in 0..(MAX_LIST + 2) {
            task.add_verified_fact(format!("f{i}"));
        }
        assert_eq!(task.verified_facts.len(), MAX_LIST);
        assert_eq!(task.verified_facts.last().unwrap(), "f13");
        assert!(!task.verified_facts.contains(&"f0".to_string()));
    }

    #[test]
    fn the_same_fact_twice_is_one_fact() {
        let mut task = TaskState::new("task_1", "o");
        task.add_verified_fact("ERC is 0");
        task.add_verified_fact("ERC is 0");
        task.attach_evidence("kicad://diff/1");
        task.attach_evidence("kicad://diff/1");
        assert_eq!(task.verified_facts.len(), 1);
        assert_eq!(task.evidence.len(), 1);
    }

    #[test]
    fn an_identical_failure_is_not_recorded_twice() {
        let mut task = TaskState::new("task_1", "o");
        task.add_failed_attempt("place C7 at 100,80", "collides with U1");
        task.add_failed_attempt("place C7 at 100,80", "collides with U1");
        assert_eq!(task.failed_attempts.len(), 1);
    }

    #[test]
    fn a_revision_note_replaces_rather_than_accumulates() {
        let mut task = TaskState::new("task_1", "o");
        task.note_revision("a.kicad_sch", "rev-1");
        task.note_revision("a.kicad_sch", "rev-2");
        assert_eq!(task.revisions["a.kicad_sch"], "rev-2");
        assert_eq!(task.revisions.len(), 1);
    }

    #[test]
    fn the_store_hands_out_stable_ids_and_bounds_itself() {
        let store = TaskStore::default();
        let first = store.start("first");
        assert_eq!(first.id, "task_1");
        assert_eq!(store.start("second").id, "task_2");
        for i in 0..STORE_CAPACITY {
            store.start(&format!("filler {i}"));
        }
        assert!(store.get("task_1").is_none(), "oldest must be evicted");
        assert!(store.list().len() <= STORE_CAPACITY);
    }

    #[test]
    fn updating_an_unknown_task_is_an_error_not_a_new_task() {
        let store = TaskStore::default();
        let err = store.update("task_99", |_| Ok(())).unwrap_err();
        assert_eq!(err.code(), "unknown_task");
        assert!(store.list().is_empty());
    }

    #[test]
    fn an_update_returns_the_task_as_it_now_is() {
        let store = TaskStore::default();
        let task = store.start("objective");
        let (updated, ()) = store
            .update(&task.id, |t| {
                t.subgoal = Some("decoupling".to_string());
                t.add_constraint("ERC = 0")
            })
            .unwrap();
        assert_eq!(updated.subgoal.as_deref(), Some("decoupling"));
        assert_eq!(store.get(&task.id).unwrap().constraints, vec!["ERC = 0"]);
    }

    #[test]
    fn an_open_task_is_the_one_still_expecting_work() {
        assert!(TaskStatus::Active.is_open());
        assert!(TaskStatus::Blocked.is_open());
        assert!(!TaskStatus::Done.is_open());
        assert!(!TaskStatus::NeedsReview.is_open());
        assert!(!TaskStatus::Abandoned.is_open());
    }
}
