//! Whole-directory before-images, so a failed batch can be undone.
//!
//! A batch of edits that stops halfway leaves a project the caller never asked
//! for and cannot describe. The cheapest honest fix, for files this size, is to
//! copy them before touching anything and write them back if the batch fails.
//!
//! Capture is by *directory*, not by the paths named in the request. A tool
//! that writes a sheet the caller never mentioned — a hierarchical child, a
//! `.kicad_pro` updated as a side effect — would otherwise survive a rollback
//! and leave the project inconsistent in the one case rollback exists for.
//!
//! Everything here is bounded: past [`SnapshotLimits`] the snapshot refuses
//! rather than reading an unbounded tree into memory, and the caller is told it
//! is running without a net instead of being quietly told it has one.

use crate::revision::DocState;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// File suffixes worth protecting: KiCAD's own design documents. Generated
/// output (gerbers, PDFs, BOM CSVs) is deliberately absent — re-running an
/// export is cheap, and pulling it in makes every snapshot pay for a `gerbers/`
/// directory it did not need.
pub const TRACKED_EXTENSIONS: &[&str] = &[
    "kicad_sch",
    "kicad_pcb",
    "kicad_pro",
    "kicad_prl",
    "kicad_sym",
    "kicad_mod",
    "kicad_dru",
    "kicad_wks",
    "net",
];

/// Directory names never descended into.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "__pycache__"];

/// Bounds on what a snapshot is willing to read.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotLimits {
    /// Maximum number of tracked files.
    pub max_files: usize,
    /// Maximum total bytes held in memory.
    pub max_bytes: u64,
    /// Maximum directory depth below each root.
    pub max_depth: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        // A KiCAD project with 40 hierarchical sheets is a big one, and its
        // documents together are a few megabytes. 400 files / 32 MiB leaves an
        // order of magnitude of headroom while still refusing a home directory.
        Self {
            max_files: 400,
            max_bytes: 32 * 1024 * 1024,
            max_depth: 6,
        }
    }
}

/// Why a snapshot could not be taken.
#[derive(Debug)]
pub enum SnapshotError {
    /// More tracked files than [`SnapshotLimits::max_files`].
    TooManyFiles { found: usize, limit: usize },
    /// More bytes than [`SnapshotLimits::max_bytes`].
    TooLarge { bytes: u64, limit: u64 },
    /// The filesystem refused a read the snapshot needed.
    Io(std::io::Error),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyFiles { found, limit } => {
                write!(f, "{found} tracked files exceeds the limit of {limit}")
            }
            Self::TooLarge { bytes, limit } => {
                write!(f, "{bytes} bytes exceeds the limit of {limit}")
            }
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl SnapshotError {
    /// Stable identifier, so a caller can branch without parsing prose.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooManyFiles { .. } => "too_many_files",
            Self::TooLarge { .. } => "too_large",
            Self::Io(_) => "io",
        }
    }
}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What a rollback actually did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Files written back to their captured content.
    pub restored: Vec<PathBuf>,
    /// Files that did not exist at capture and were removed.
    pub deleted: Vec<PathBuf>,
    /// Paths that could not be put back, with the reason. A rollback that
    /// half-worked must say so — reporting a clean undo it did not perform is
    /// worse than the original failure.
    pub failed: Vec<(PathBuf, String)>,
}

impl RestoreReport {
    /// Whether anything at all was changed back.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.restored.is_empty() && self.deleted.is_empty() && self.failed.is_empty()
    }
}

/// A before-image of every tracked file under a set of roots.
#[derive(Debug)]
pub struct Snapshot {
    roots: Vec<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    limits: SnapshotLimits,
}

impl Snapshot {
    /// Read every tracked file under `roots`.
    ///
    /// Roots that do not exist are skipped rather than refused: creating a
    /// project is a legitimate batch, and its directory appears while the batch
    /// runs.
    pub fn capture(roots: &[PathBuf], limits: SnapshotLimits) -> Result<Self, SnapshotError> {
        let roots = normalise_roots(roots);
        let mut paths = Vec::new();
        for root in &roots {
            collect(root, 0, limits, &mut paths)?;
        }
        if paths.len() > limits.max_files {
            return Err(SnapshotError::TooManyFiles {
                found: paths.len(),
                limit: limits.max_files,
            });
        }

        let mut files = BTreeMap::new();
        let mut total: u64 = 0;
        for path in paths {
            let bytes = std::fs::read(&path)?;
            total += bytes.len() as u64;
            if total > limits.max_bytes {
                return Err(SnapshotError::TooLarge {
                    bytes: total,
                    limit: limits.max_bytes,
                });
            }
            files.insert(path, bytes);
        }

        Ok(Self {
            roots,
            files,
            limits,
        })
    }

    /// Number of files held.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// The bytes captured for a path, if it existed when the snapshot was
    /// taken.
    ///
    /// This is what makes the snapshot a *before image* rather than only an
    /// undo buffer: whoever understands the file format can compare these bytes
    /// with what is on disk now and say what changed in domain terms. `None`
    /// means the file was created during the batch.
    #[must_use]
    pub fn before(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Paths whose content differs from the capture, with their current state.
    ///
    /// Covers all three transitions — modified, created, deleted — because the
    /// caller uses this to report "here is the revision you should pass as
    /// `base_revisions` next time", and a deleted file has a state too.
    pub fn changed(&self) -> Vec<(PathBuf, DocState)> {
        let mut out = Vec::new();
        for (path, before) in &self.files {
            match DocState::read(path) {
                Ok(now) => {
                    if now != DocState::of_bytes(before) {
                        out.push((path.clone(), now));
                    }
                }
                Err(_) => continue,
            }
        }
        for path in self.current_tracked_files() {
            if !self.files.contains_key(&path) {
                if let Ok(now) = DocState::read(&path) {
                    out.push((path, now));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Put every tracked file back the way it was.
    ///
    /// Files whose content already matches are not rewritten: an untouched
    /// document should keep its mtime so KiCAD does not offer to reload it.
    pub fn restore(&self) -> RestoreReport {
        let mut report = RestoreReport::default();

        for (path, before) in &self.files {
            match std::fs::read(path) {
                Ok(now) if now == *before => continue,
                _ => {}
            }
            match write_atomic(path, before) {
                Ok(()) => report.restored.push(path.clone()),
                Err(e) => report.failed.push((path.clone(), e.to_string())),
            }
        }

        for path in self.current_tracked_files() {
            if self.files.contains_key(&path) {
                continue;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => report.deleted.push(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => report.failed.push((path, e.to_string())),
            }
        }

        report.restored.sort();
        report.deleted.sort();
        report
    }

    fn current_tracked_files(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for root in &self.roots {
            // A rescan that hits the limit is not a reason to abandon the
            // rollback: what was collected before the limit is still worth
            // putting back.
            let _ = collect(root, 0, self.limits, &mut paths);
        }
        paths.sort();
        paths.dedup();
        paths
    }
}

/// Drop roots contained in another root so a nested pair is not walked twice.
fn normalise_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::new();
    for root in roots {
        let root = root.clone();
        if unique.iter().any(|kept| root.starts_with(kept)) {
            continue;
        }
        unique.retain(|kept| !kept.starts_with(&root));
        unique.push(root);
    }
    unique.sort();
    unique
}

fn collect(
    dir: &Path,
    depth: usize,
    limits: SnapshotLimits,
    out: &mut Vec<PathBuf>,
) -> Result<(), SnapshotError> {
    if depth > limits.max_depth {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(SnapshotError::Io(e)),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) || name.ends_with("-backups") {
                continue;
            }
            collect(&path, depth + 1, limits, out)?;
        } else if file_type.is_file() && is_tracked(&path) {
            out.push(path);
            if out.len() > limits.max_files {
                return Err(SnapshotError::TooManyFiles {
                    found: out.len(),
                    limit: limits.max_files,
                });
            }
        }
    }
    Ok(())
}

fn is_tracked(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| TRACKED_EXTENSIONS.contains(&ext))
}

/// Write through a sibling temporary file, then rename over the target. A
/// rollback interrupted mid-write must not be the thing that corrupts the
/// document it was rescuing.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("doc");
    let tmp = dir.join(format!(".{name}.kam-restore-{stamp}"));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn restore_undoes_modification_and_creation() {
        let dir = tempfile::tempdir().unwrap();
        let sch = write(dir.path(), "a.kicad_sch", "(kicad_sch original)");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        assert_eq!(snap.file_count(), 1);

        std::fs::write(&sch, "(kicad_sch edited)").unwrap();
        let created = write(dir.path(), "b.kicad_sch", "(kicad_sch new)");

        let report = snap.restore();
        assert_eq!(report.restored, vec![sch.clone()]);
        assert_eq!(report.deleted, vec![created.clone()]);
        assert!(report.failed.is_empty());
        assert_eq!(
            std::fs::read_to_string(&sch).unwrap(),
            "(kicad_sch original)"
        );
        assert!(!created.exists());
    }

    #[test]
    fn restore_brings_back_a_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let sch = write(dir.path(), "a.kicad_sch", "(kicad_sch original)");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        std::fs::remove_file(&sch).unwrap();

        let report = snap.restore();
        assert_eq!(report.restored, vec![sch.clone()]);
        assert_eq!(
            std::fs::read_to_string(&sch).unwrap(),
            "(kicad_sch original)"
        );
    }

    #[test]
    fn untracked_files_are_neither_captured_nor_deleted() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.kicad_sch", "(kicad_sch original)");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        let gerber = write(dir.path(), "out.gbr", "G04*");

        let report = snap.restore();
        assert!(report.deleted.is_empty());
        assert!(
            gerber.exists(),
            "generated output is not the snapshot's business"
        );
    }

    #[test]
    fn nested_sheets_are_captured() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "top.kicad_sch", "top");
        let child = write(dir.path(), "sheets/child.kicad_sch", "child");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        assert_eq!(snap.file_count(), 2);

        std::fs::write(&child, "edited").unwrap();
        assert_eq!(snap.changed().len(), 1);
        snap.restore();
        assert_eq!(std::fs::read_to_string(&child).unwrap(), "child");
    }

    #[test]
    fn changed_reports_the_current_revision_of_each_touched_file() {
        let dir = tempfile::tempdir().unwrap();
        let sch = write(dir.path(), "a.kicad_sch", "before");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        assert!(snap.changed().is_empty());

        std::fs::write(&sch, "after").unwrap();
        let changed = snap.changed();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, sch);
        assert_eq!(changed[0].1, DocState::of_bytes(b"after"));
    }

    #[test]
    fn the_capture_is_readable_as_a_before_image() {
        let dir = tempfile::tempdir().unwrap();
        let sch = write(dir.path(), "a.kicad_sch", "before");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        std::fs::write(&sch, "after").unwrap();

        assert_eq!(snap.before(&sch), Some(b"before".as_slice()));
        assert_eq!(
            snap.before(&dir.path().join("never.kicad_sch")),
            None,
            "a file created during the batch has no before image"
        );
    }

    #[test]
    fn a_missing_root_captures_nothing_and_deletes_what_appears() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("not-yet");
        let snap =
            Snapshot::capture(std::slice::from_ref(&root), SnapshotLimits::default()).unwrap();
        assert_eq!(snap.file_count(), 0);

        let created = write(&root, "new.kicad_pro", "{}");
        let report = snap.restore();
        assert_eq!(report.deleted, vec![created.clone()]);
        assert!(!created.exists());
    }

    #[test]
    fn the_file_limit_refuses_instead_of_reading_everything() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(dir.path(), &format!("s{i}.kicad_sch"), "x");
        }
        let limits = SnapshotLimits {
            max_files: 3,
            ..SnapshotLimits::default()
        };
        let err = Snapshot::capture(&[dir.path().to_path_buf()], limits).unwrap_err();
        assert_eq!(err.code(), "too_many_files");
    }

    #[test]
    fn the_byte_limit_refuses_instead_of_reading_everything() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "big.kicad_pcb", &"x".repeat(4096));
        let limits = SnapshotLimits {
            max_bytes: 128,
            ..SnapshotLimits::default()
        };
        let err = Snapshot::capture(&[dir.path().to_path_buf()], limits).unwrap_err();
        assert_eq!(err.code(), "too_large");
    }

    #[test]
    fn nested_roots_are_walked_once() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "sub/a.kicad_sch", "a");
        let roots = vec![dir.path().to_path_buf(), dir.path().join("sub")];
        let snap = Snapshot::capture(&roots, SnapshotLimits::default()).unwrap();
        assert_eq!(snap.file_count(), 1);
    }

    #[test]
    fn backup_directories_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.kicad_sch", "a");
        write(dir.path(), "a-backups/a.kicad_sch", "old");
        let snap =
            Snapshot::capture(&[dir.path().to_path_buf()], SnapshotLimits::default()).unwrap();
        assert_eq!(
            snap.file_count(),
            1,
            "KiCAD's own backups are not our state"
        );
    }
}
