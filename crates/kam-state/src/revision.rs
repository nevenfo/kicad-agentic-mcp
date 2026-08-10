//! Content-addressed document revisions.
//!
//! A revision is a function of the bytes on disk, never of a counter held in
//! memory. That is the only definition that survives the case this exists for:
//! the user edits the file in KiCAD while an agent is holding a plan. An
//! in-process counter would happily report "unchanged"; a content hash cannot.
//!
//! The token format is `<16 hex>-<len>`, short enough to pass back and forth in
//! a tool payload without turning into a token cost of its own. The length is
//! carried alongside the digest because it makes a truncated-digest collision
//! need to also preserve file size, and because it is free.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

/// Token used for a path that does not exist.
///
/// A missing file is a state a precondition must be able to name: "create this
/// project only if nothing is there yet" is as much a precondition as "edit
/// this schematic only if it still looks like this".
pub const ABSENT: &str = "absent";

/// The revision of one document's content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Revision {
    digest: String,
    len: u64,
}

impl Revision {
    /// Revision of an in-memory byte string.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self {
            digest: hex16(&digest),
            len: bytes.len() as u64,
        }
    }

    /// Stable token, e.g. `3f8a1c02de4b5670-20481`.
    #[must_use]
    pub fn token(&self) -> String {
        format!("{}-{}", self.digest, self.len)
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.digest, self.len)
    }
}

/// Whether a document exists, and if so at which revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocState {
    /// No file at this path.
    Absent,
    /// A file whose content hashes to this revision.
    Present(Revision),
}

impl DocState {
    /// Read the current state of `path`.
    ///
    /// A missing path is `Absent`, not an error: callers ask this question
    /// precisely because they do not know yet.
    pub fn read(path: &Path) -> std::io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Self::Present(Revision::of_bytes(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(e) => Err(e),
        }
    }

    /// State of already-read bytes.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::Present(Revision::of_bytes(bytes))
    }

    /// Comparable token: the revision, or `absent`.
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::Absent => ABSENT.to_string(),
            Self::Present(r) => r.token(),
        }
    }
}

impl fmt::Display for DocState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => f.write_str(ABSENT),
            Self::Present(r) => write!(f, "{r}"),
        }
    }
}

fn hex16(digest: &[u8]) -> String {
    let mut s = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_is_deterministic_and_content_sensitive() {
        let a = Revision::of_bytes(b"(kicad_sch (version 20260306))");
        let b = Revision::of_bytes(b"(kicad_sch (version 20260306))");
        let c = Revision::of_bytes(b"(kicad_sch (version 20260307))");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.token(), b.token());
    }

    #[test]
    fn token_carries_digest_and_length() {
        let r = Revision::of_bytes(b"1234567890");
        let token = r.token();
        let (digest, len) = token.rsplit_once('-').unwrap();
        assert_eq!(digest.len(), 16);
        assert_eq!(len, "10");
    }

    #[test]
    fn same_length_different_content_differs() {
        // Length alone is not a revision — the digest has to be doing the work.
        let a = Revision::of_bytes(b"aaaa");
        let b = Revision::of_bytes(b"aaab");
        assert_ne!(a.token(), b.token());
    }

    #[test]
    fn missing_file_reads_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.kicad_sch");
        assert_eq!(DocState::read(&path).unwrap(), DocState::Absent);
        assert_eq!(DocState::read(&path).unwrap().token(), "absent");
    }

    #[test]
    fn file_state_follows_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.kicad_sch");
        std::fs::write(&path, b"one").unwrap();
        let first = DocState::read(&path).unwrap();
        std::fs::write(&path, b"two").unwrap();
        let second = DocState::read(&path).unwrap();
        assert_ne!(first.token(), second.token());
        assert_eq!(second, DocState::of_bytes(b"two"));
    }
}
