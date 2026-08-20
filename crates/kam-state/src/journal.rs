//! Append-only record of what a batch did, independent of the rollback
//! [`crate::snapshot::Snapshot`] takes to make an in-flight batch undoable.
//!
//! A [`Snapshot`](crate::snapshot::Snapshot) answers "can I undo what I just
//! half-did?" and then it is gone — the process holding it drops it once the
//! batch returns. Nothing else answers "what did the last ten batches do, and
//! can I look at what they changed after the fact?" That is what this module
//! is for: one JSON line per batch that changed something, plus, budget
//! permitting, the before/after bytes of the documents it touched.
//!
//! Deliberately domain-free, like the rest of this crate: a [`RunRecord`] is
//! built from paths and bytes the caller already has, and this module never
//! reads a `.kicad_sch` to find out what changed in it. It also never guesses
//! *where* to live — [`RunJournal::open`] takes a directory from the caller,
//! rather than resolving a platform config directory itself, so a test binary
//! and a production server can each point it at their own place without this
//! crate knowing the difference (see `plan.md`, "License impact": this crate
//! stays clean-room by not knowing what KiCAD is, and by not knowing what a
//! user's profile directory is either).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Bounds on what the journal is willing to keep.
///
/// Three independent budgets rather than one, because a journal that has run
/// for months and a journal that just replayed a hundred-file import fail in
/// different ways: the first needs a line-count cap so `journal.jsonl` itself
/// stays boring to read, the second needs a byte cap so one entry cannot spend
/// the whole image budget alone.
#[derive(Debug, Clone, Copy)]
pub struct JournalLimits {
    /// Lines kept in `journal.jsonl`. 512 is a few weeks of ordinary editing
    /// sessions at a few dozen batches a day — enough history to answer "what
    /// happened recently" without the file becoming something a caller has to
    /// think about streaming.
    pub max_entries: usize,
    /// Image sets (a `pre/` and/or `post/` directory per entry) kept on disk.
    /// 16 is a working day's worth of undo-able batches; older ones keep their
    /// line in `journal.jsonl` — the record of *what* happened — but lose the
    /// bytes needed to replay it.
    pub max_image_sets: usize,
    /// Total bytes across every image set. 64 MiB is a couple of full-project
    /// snapshots' worth (see `SnapshotLimits::default`'s 32 MiB per capture),
    /// which is plenty for images that are themselves scoped to only the files
    /// a batch actually touched rather than a whole project.
    pub max_image_bytes: u64,
    /// Above this many bytes of before+after content, an entry is written
    /// without images. A single batch that rewrites a multi-megabyte board is
    /// legitimate design work; letting it alone consume the entire image
    /// budget and evict everything before it is not.
    pub max_entry_bytes: u64,
}

impl Default for JournalLimits {
    fn default() -> Self {
        Self {
            max_entries: 512,
            max_image_sets: 16,
            max_image_bytes: 64 * 1024 * 1024,
            max_entry_bytes: 8 * 1024 * 1024,
        }
    }
}

/// What became of a batch, from the journal's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The batch's edits are on disk.
    Applied,
    /// The batch failed and its edits were undone.
    RolledBack,
}

/// One document a batch touched, as the caller already has it: a path and the
/// revisions either side of the change. Bytes are optional even though the
/// revisions are not — a caller with a revision but no bytes still gets a
/// entry, just one without images to replay.
pub struct RecordedDoc<'a> {
    pub path: &'a Path,
    pub before_revision: Option<&'a str>,
    pub after_revision: &'a str,
    pub before_bytes: Option<&'a [u8]>,
    pub after_bytes: Option<&'a [u8]>,
}

/// What a caller reports to [`RunJournal::append`] after a batch finishes.
pub struct RunRecord<'a> {
    pub outcome: Outcome,
    /// Tool names called, in order, for a reader who wants "what happened"
    /// without opening the full result payload.
    pub tools: &'a [String],
    pub documents: Vec<RecordedDoc<'a>>,
    pub operation_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    /// The semantic diff summary the batch already computed, if it computed
    /// one — the journal never recomputes it, only carries it.
    pub summary: Option<&'a str>,
}

/// One document as it was written to `journal.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    /// Relative to [`JournalEntry::root`] when one is present, otherwise as
    /// the caller supplied it.
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

/// Which side of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSide {
    Pre,
    Post,
}

/// One line of `journal.jsonl`, deserialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// 12 hex characters, stable, and also the name of this entry's image
    /// directory under `images/` — one identifier instead of two so a reader
    /// never has to carry both.
    pub id: String,
    /// Unix epoch milliseconds.
    pub ts: u64,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub tools: Vec<String>,
    pub documents: Vec<DocumentEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The directory `documents[].path` is relative to, itself relative to
    /// the journal's own directory — never absolute (see the module doc and
    /// `konnect-core`'s `snapshot_manifest`, decision D72, which made the same
    /// call for the same reason: an absolute path buys no audit value and
    /// leaks the caller's filesystem layout into a file meant to be portable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// `images/<id>/pre`, relative to the journal directory, or `None` when no
    /// before-image is there to read — never captured, or captured and since
    /// evicted. [`RunJournal::entries`] resolves the second case by looking,
    /// so what a reader holds is true of the disk now rather than of the disk
    /// at the moment the line was appended.
    pub pre_snapshot_path: Option<String>,
    /// `images/<id>/post`, same rules.
    pub post_snapshot_path: Option<String>,
    /// `"too_large"` when this entry's documents exceeded
    /// [`JournalLimits::max_entry_bytes`] and images were never written for
    /// it; `None` otherwise. Distinguished from a plain absent image so a
    /// reader can tell "never had one" from "budget removed it later" for the
    /// entries where that distinction was known at write time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images_omitted: Option<String>,
    /// Present if and only if the pre-image is on disk — checked on the way
    /// out of [`RunJournal::entries`], not merely on the way in, because a
    /// token naming bytes the budget has already deleted is a name for a
    /// restore point that does not exist, and a field is only worth having
    /// while it is true. Names a restore point; it is not a capability, and nothing in
    /// MCP accepts it — rollback stays internal to the batch that ran it
    /// (plan.md D12/D.2.2). Never copy this into an MCP response: publishing
    /// an address no tool accepts would invite a caller to go looking for an
    /// operation that does not exist.
    pub rollback_token: Option<String>,
}

/// An append-only journal of run outcomes, with bounded before/after images.
pub struct RunJournal {
    dir: PathBuf,
    limits: JournalLimits,
    /// Serializes `append` end to end — claiming an id, writing images,
    /// appending the line, and applying retention all need to happen as one
    /// step, or two concurrent batches could interleave a half-written image
    /// set with another's eviction pass.
    write_lock: Mutex<()>,
}

impl RunJournal {
    /// Open (creating if needed) the journal rooted at `dir`, with default
    /// limits.
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::with_limits(dir, JournalLimits::default())
    }

    /// Same, with caller-chosen limits — mainly for tests that want small
    /// budgets to exercise eviction without writing megabytes.
    pub fn with_limits(dir: impl Into<PathBuf>, limits: JournalLimits) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        std::fs::create_dir_all(dir.join("images"))?;
        Ok(Self {
            dir,
            limits,
            write_lock: Mutex::new(()),
        })
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn journal_path(&self) -> PathBuf {
        self.dir.join("journal.jsonl")
    }

    fn images_dir(&self) -> PathBuf {
        self.dir.join("images")
    }

    /// Append one entry: copy whatever images the budget allows, write the
    /// line, then bring the journal back within its retention limits.
    ///
    /// Never panics. IO failures anywhere in the process are returned to the
    /// caller rather than swallowed here — this crate has no logging facility
    /// of its own, so "best effort, and tell someone" is `konnect-core`'s job
    /// at the call site, not this one's.
    pub fn append(&self, record: RunRecord<'_>) -> std::io::Result<String> {
        let _guard = self.lock();

        let id = new_id();
        let ts = unix_ms();

        let doc_paths: Vec<&Path> = record.documents.iter().map(|d| d.path).collect();
        let root = common_root(&doc_paths);

        let entry_bytes: u64 = record
            .documents
            .iter()
            .map(|d| {
                d.before_bytes.map_or(0, |b| b.len() as u64)
                    + d.after_bytes.map_or(0, |b| b.len() as u64)
            })
            .sum();

        let mut documents = Vec::with_capacity(record.documents.len());
        let mut rel_paths = Vec::with_capacity(record.documents.len());
        for doc in &record.documents {
            let rel = relativize(doc.path, root.as_deref());
            documents.push(DocumentEntry {
                path: path_to_slash_string(&rel),
                before: doc.before_revision.map(str::to_string),
                after: doc.after_revision.to_string(),
            });
            rel_paths.push(rel);
        }

        let mut images_omitted = None;
        let mut pre_snapshot_path = None;
        let mut post_snapshot_path = None;
        let mut rollback_token = None;

        if entry_bytes > self.limits.max_entry_bytes {
            images_omitted = Some("too_large".to_string());
        } else {
            let set_dir = self.images_dir().join(&id);
            let mut wrote_pre = false;
            let mut wrote_post = false;
            for (doc, rel) in record.documents.iter().zip(rel_paths.iter()) {
                if let Some(bytes) = doc.before_bytes {
                    write_image_file(&set_dir.join("pre").join(rel), bytes)?;
                    wrote_pre = true;
                }
                if let Some(bytes) = doc.after_bytes {
                    write_image_file(&set_dir.join("post").join(rel), bytes)?;
                    wrote_post = true;
                }
            }
            if wrote_pre {
                pre_snapshot_path = Some(format!("images/{id}/pre"));
                rollback_token = Some(id.clone());
            }
            if wrote_post {
                post_snapshot_path = Some(format!("images/{id}/post"));
            }
        }

        let entry = JournalEntry {
            id: id.clone(),
            ts,
            outcome: record.outcome,
            operation_id: record.operation_id.map(str::to_string),
            task_id: record.task_id.map(str::to_string),
            tools: record.tools.to_vec(),
            documents,
            summary: record.summary.map(str::to_string),
            root: root.map(|r| path_to_slash_string(&r)),
            pre_snapshot_path,
            post_snapshot_path,
            images_omitted,
            rollback_token,
        };

        let line = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.journal_path())?;
            writeln!(file, "{line}")?;
            file.flush()?;
        }

        self.compact_entries()?;
        self.evict_images()?;

        Ok(id)
    }

    /// Every entry, oldest first. A line that does not parse is skipped, not
    /// an error — a journal with one corrupt line (a crash mid-write, say) is
    /// still worth reading for every line it does have.
    pub fn entries(&self) -> std::io::Result<Vec<JournalEntry>> {
        let text = match std::fs::read_to_string(self.journal_path()) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<JournalEntry>(l).ok())
            .map(|mut entry| {
                self.forget_evicted_images(&mut entry);
                entry
            })
            .collect())
    }

    /// Clear the image fields of an entry whose image set the budget has since
    /// deleted.
    ///
    /// The line on disk is never rewritten for this: it is append-only, and
    /// eviction is a budget decision rather than a correction to what
    /// happened. The truth is re-established on the read side instead, at the
    /// cost of one directory check per entry — which is also the only place it
    /// *can* be established, since the next eviction can happen at any time
    /// after a line is written.
    fn forget_evicted_images(&self, entry: &mut JournalEntry) {
        if self.images_dir().join(&entry.id).is_dir() {
            return;
        }
        entry.pre_snapshot_path = None;
        entry.post_snapshot_path = None;
        entry.rollback_token = None;
    }

    /// The before/after bytes of one document in one entry, read from the
    /// images captured for it. `None` when there never was an image (the
    /// entry had none, the document had no bytes on that side, or the budget
    /// evicted the file since) — the three cases collapse to the same answer
    /// because a caller acting on the bytes cannot tell them apart anyway.
    #[must_use]
    pub fn image(
        &self,
        entry: &JournalEntry,
        doc_index: usize,
        side: ImageSide,
    ) -> Option<Vec<u8>> {
        let doc = entry.documents.get(doc_index)?;
        let base = match side {
            ImageSide::Pre => entry.pre_snapshot_path.as_ref()?,
            ImageSide::Post => entry.post_snapshot_path.as_ref()?,
        };
        let path = self.dir.join(base).join(slash_string_to_path(&doc.path));
        std::fs::read(path).ok()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rewrite `journal.jsonl` to its last [`JournalLimits::max_entries`]
    /// lines, atomically, when it has grown past that. Raw lines, not
    /// re-serialized entries — a line this pass cannot parse is kept as-is
    /// rather than dropped, since dropping it is a second way to lose history
    /// on top of whatever already made it unparseable.
    fn compact_entries(&self) -> std::io::Result<()> {
        let path = self.journal_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() <= self.limits.max_entries {
            return Ok(());
        }
        let kept = &lines[lines.len() - self.limits.max_entries..];
        let mut body = kept.join("\n");
        body.push('\n');
        write_atomic(&path, body.as_bytes())
    }

    /// Drop the oldest image sets until both the set-count and total-byte
    /// budgets are met. The `journal.jsonl` lines for those entries are left
    /// exactly as they were — [`Self::image`] finding nothing on disk is what
    /// tells a reader the budget took it, not a flag flipped after the fact.
    fn evict_images(&self) -> std::io::Result<()> {
        let images_dir = self.images_dir();
        if !images_dir.is_dir() {
            return Ok(());
        }
        let entries = self.entries()?; // oldest first
        let mut sets: Vec<(String, u64)> = Vec::new();
        for e in &entries {
            let set_dir = images_dir.join(&e.id);
            if set_dir.is_dir() {
                sets.push((e.id.clone(), dir_size(&set_dir)?));
            }
        }
        let mut total: u64 = sets.iter().map(|(_, s)| *s).sum();
        let mut count = sets.len();
        let mut idx = 0;
        while idx < sets.len()
            && (count > self.limits.max_image_sets || total > self.limits.max_image_bytes)
        {
            let (id, size) = &sets[idx];
            std::fs::remove_dir_all(images_dir.join(id))?;
            total = total.saturating_sub(*size);
            count -= 1;
            idx += 1;
        }
        Ok(())
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 12 hex characters, unique enough within one process: a hash of a
/// nanosecond timestamp and a monotonic counter, so two entries appended in
/// the same nanosecond (a real possibility — timers are coarser than that on
/// some platforms) still get different ids.
fn new_id() -> String {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let seq = CTR.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(seq.to_le_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// The common containing directory of every document's path, so
/// [`DocumentEntry::path`] never has to carry more than a project-relative
/// tail. `None` when a path has no parent to share (already bare, e.g. a
/// caller that passed a filename with no directory) — there is nothing to
/// factor out, so nothing is.
fn common_root(paths: &[&Path]) -> Option<PathBuf> {
    let mut dirs = paths
        .iter()
        .map(|p| p.parent().unwrap_or_else(|| Path::new("")));
    let first = dirs.next()?;
    let mut prefix: Vec<_> = first.components().collect();
    for dir in dirs {
        let comps: Vec<_> = dir.components().collect();
        let n = prefix
            .iter()
            .zip(comps.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(n);
    }
    if prefix.is_empty() {
        None
    } else {
        Some(prefix.into_iter().collect())
    }
}

fn relativize(path: &Path, root: Option<&Path>) -> PathBuf {
    match root {
        Some(root) => path.strip_prefix(root).unwrap_or(path).to_path_buf(),
        None => path.to_path_buf(),
    }
}

/// Store paths with `/` regardless of platform, so a journal written on
/// Windows reads the same on Linux — the whole point of keeping paths
/// relative in the first place.
fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn slash_string_to_path(s: &str) -> PathBuf {
    s.split('/').collect()
}

fn write_image_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

/// Write through a sibling temporary file, then rename over the target — the
/// same pattern `snapshot::write_atomic` uses, so a process killed mid-compact
/// leaves either the old `journal.jsonl` or the new one, never a half-written
/// one.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".journal.jsonl.kam-compact-{stamp}"));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn dir_size(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            total += dir_size(&entry.path())?;
        } else if file_type.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc<'a>(
        path: &'a Path,
        before: Option<&'a str>,
        after: &'a str,
        before_bytes: Option<&'a [u8]>,
        after_bytes: Option<&'a [u8]>,
    ) -> RecordedDoc<'a> {
        RecordedDoc {
            path,
            before_revision: before,
            after_revision: after,
            before_bytes,
            after_bytes,
        }
    }

    #[test]
    fn append_and_entries_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let journal = RunJournal::open(dir.path()).unwrap();
        let path = dir.path().join("proj").join("a.kicad_sch");
        let tools = vec!["place_component".to_string()];

        let id = journal
            .append(RunRecord {
                outcome: Outcome::Applied,
                tools: &tools,
                documents: vec![doc(
                    &path,
                    Some("rev0"),
                    "rev1",
                    Some(b"before"),
                    Some(b"after"),
                )],
                operation_id: Some("op_1"),
                task_id: None,
                summary: Some("symbol +1"),
            })
            .unwrap();

        let entries = journal.entries().unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.outcome, Outcome::Applied);
        assert_eq!(entry.operation_id.as_deref(), Some("op_1"));
        assert_eq!(entry.task_id, None);
        assert_eq!(entry.summary.as_deref(), Some("symbol +1"));
        assert_eq!(entry.documents[0].path, "a.kicad_sch");
        assert_eq!(entry.documents[0].before.as_deref(), Some("rev0"));
        assert_eq!(entry.documents[0].after, "rev1");
        assert!(entry.pre_snapshot_path.is_some());
        assert!(entry.post_snapshot_path.is_some());
        assert_eq!(entry.rollback_token.as_deref(), Some(id.as_str()));
        assert_eq!(entry.images_omitted, None);
    }

    #[test]
    fn images_are_read_back_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let journal = RunJournal::open(dir.path()).unwrap();
        let path = dir.path().join("a.kicad_sch");
        let tools = vec!["x".to_string()];

        journal
            .append(RunRecord {
                outcome: Outcome::Applied,
                tools: &tools,
                documents: vec![doc(&path, None, "rev1", None, Some(b"the new content"))],
                operation_id: None,
                task_id: None,
                summary: None,
            })
            .unwrap();

        let entry = &journal.entries().unwrap()[0];
        assert_eq!(
            journal.image(entry, 0, ImageSide::Pre),
            None,
            "no before image was ever offered"
        );
        assert_eq!(
            journal.image(entry, 0, ImageSide::Post),
            Some(b"the new content".to_vec())
        );
    }

    #[test]
    fn an_entry_over_the_byte_budget_is_written_without_images() {
        let dir = tempfile::tempdir().unwrap();
        let limits = JournalLimits {
            max_entry_bytes: 4,
            ..JournalLimits::default()
        };
        let journal = RunJournal::with_limits(dir.path(), limits).unwrap();
        let path = dir.path().join("a.kicad_sch");
        let tools = vec!["x".to_string()];

        journal
            .append(RunRecord {
                outcome: Outcome::Applied,
                tools: &tools,
                documents: vec![doc(
                    &path,
                    None,
                    "rev1",
                    None,
                    Some(b"more than four bytes"),
                )],
                operation_id: None,
                task_id: None,
                summary: None,
            })
            .unwrap();

        let entry = &journal.entries().unwrap()[0];
        assert_eq!(entry.images_omitted.as_deref(), Some("too_large"));
        assert_eq!(entry.pre_snapshot_path, None);
        assert_eq!(entry.post_snapshot_path, None);
        assert_eq!(entry.rollback_token, None);
    }

    #[test]
    fn eviction_removes_the_oldest_image_sets_but_keeps_the_lines() {
        let dir = tempfile::tempdir().unwrap();
        let limits = JournalLimits {
            max_image_sets: 2,
            ..JournalLimits::default()
        };
        let journal = RunJournal::with_limits(dir.path(), limits).unwrap();
        let tools = vec!["x".to_string()];

        let mut ids = Vec::new();
        for i in 0..3 {
            let path = dir.path().join(format!("a{i}.kicad_sch"));
            let id = journal
                .append(RunRecord {
                    outcome: Outcome::Applied,
                    tools: &tools,
                    documents: vec![doc(
                        &path,
                        Some("rev0"),
                        "rev1",
                        Some(b"before"),
                        Some(b"content"),
                    )],
                    operation_id: None,
                    task_id: None,
                    summary: None,
                })
                .unwrap();
            ids.push(id);
        }

        let entries = journal.entries().unwrap();
        assert_eq!(entries.len(), 3, "every line survives eviction");
        assert_eq!(
            entries[0].post_snapshot_path, None,
            "an entry whose images the budget took reads as having none"
        );
        assert_eq!(
            entries[0].rollback_token, None,
            "and names no restore point, since the bytes to restore are gone"
        );
        assert_eq!(
            journal.image(&entries[0], 0, ImageSide::Post),
            None,
            "the oldest image set was evicted"
        );
        assert_eq!(
            journal.image(&entries[2], 0, ImageSide::Post),
            Some(b"content".to_vec()),
            "the newest image set survives"
        );
        assert_eq!(
            entries[2].rollback_token.as_deref(),
            Some(ids[2].as_str()),
            "and still names its restore point"
        );
    }

    #[test]
    fn compaction_past_max_entries_keeps_the_most_recent_lines() {
        let dir = tempfile::tempdir().unwrap();
        let limits = JournalLimits {
            max_entries: 2,
            ..JournalLimits::default()
        };
        let journal = RunJournal::with_limits(dir.path(), limits).unwrap();
        let tools = vec!["x".to_string()];

        for i in 0..4 {
            let path = dir.path().join(format!("a{i}.kicad_sch"));
            journal
                .append(RunRecord {
                    outcome: Outcome::Applied,
                    tools: &tools,
                    documents: vec![doc(&path, None, "rev1", None, None)],
                    operation_id: None,
                    task_id: None,
                    summary: Some(&format!("batch {i}")),
                })
                .unwrap();
        }

        let entries = journal.entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].summary.as_deref(), Some("batch 2"));
        assert_eq!(entries[1].summary.as_deref(), Some("batch 3"));
    }
}
