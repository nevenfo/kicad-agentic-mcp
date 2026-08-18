//! Safety primitives for batched mutations of on-disk documents.
//!
//! Three questions a batch executor has to answer, and nothing else:
//!
//! * **"Is the document still what the plan was written against?"**
//!   [`revision`] — content-addressed revisions, so a user editing the file in
//!   another window is detected instead of overwritten.
//! * **"Have I already run this?"** [`ledger`] — an idempotency key, so a retry
//!   after a timeout returns the first result rather than applying it twice.
//! * **"Can I undo what I just half-did?"** [`snapshot`] — before-images of a
//!   directory, written back when a batch fails partway.
//!
//! And one a batch executor cannot answer alone: **"what was I doing?"**
//! [`task`] — objective, constraints, verified facts and failed attempts held
//! outside any model's context, so a compaction or a model swap does not lose
//! them.
//!
//! One more question, orthogonal to all four: **"am I even allowed to write
//! right now?"** [`mode`] — the process-wide operating mode, independent of
//! which toolset a client has loaded.
//!
//! Deliberately domain-free: this crate has no idea what a schematic is, and
//! depends on no `konnect-*` type. It is one of the `kam-*` crates kept
//! clean-room so the base project underneath can change without rewriting them
//! (see `plan.md`, "License impact").

pub mod ledger;
pub mod mode;
pub mod revision;
pub mod snapshot;
pub mod task;

pub use ledger::{Claim, IdempotencyLedger};
pub use mode::{ModeGuard, ModeParseError, OperatingMode};
pub use revision::{DocState, Revision, ABSENT};
pub use snapshot::{RestoreReport, Snapshot, SnapshotError, SnapshotLimits, TRACKED_EXTENSIONS};
pub use task::{FailedAttempt, Progress, TaskError, TaskState, TaskStatus, TaskStore, TaskSummary};
