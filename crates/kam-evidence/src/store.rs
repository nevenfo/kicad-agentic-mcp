//! Keeping the detail out of the reply.
//!
//! A batch reply has to answer two different readers. The agent that made the
//! call needs one line — `symbol +2, wire ~1` — and pays for every token of it
//! on every subsequent turn. A reviewer challenging the change needs every
//! attribute of every item, and needs it exactly once.
//!
//! Putting both in the reply serves the second reader by taxing the first. So
//! the detail is stored here and the reply carries a handle to it: a URI the
//! caller resolves through MCP `resources/read` only when it decides to look.
//!
//! The store is in memory and bounded, by entry count and by serialized size.
//! That bound is the reason a lookup distinguishes *evicted* from *never
//! existed*: a handle that has aged out must say so, because "not found" would
//! read as "the server is lying about what it did".

use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

/// How many entries are retained before the oldest is dropped.
const DEFAULT_CAPACITY: usize = 64;

/// Total serialized size retained, in bytes. One re-route diff can be large;
/// forty of them must not be a memory leak with a URI scheme.
const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;

/// A stored artefact and the URI that fetches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handle {
    /// Resolvable URI, e.g. `kicad://diff/12`.
    pub uri: String,
    /// Monotonic id, unique across kinds for the life of the process.
    pub id: u64,
}

/// One artefact, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Monotonic id.
    pub id: u64,
    /// What sort of artefact this is — the middle segment of the URI.
    pub kind: String,
    /// One line describing the body, shown in listings so a reader can choose
    /// without fetching.
    pub summary: String,
    /// The artefact itself.
    pub body: Value,
    /// Serialized size of `body`, counted once at insertion.
    pub bytes: usize,
}

impl Entry {
    /// The URI this entry answers to.
    #[must_use]
    pub fn uri(&self, scheme: &str) -> String {
        format!("{scheme}://{}/{}", self.kind, self.id)
    }
}

/// Why a handle did not resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupError {
    /// Not a URI this store issues.
    Malformed,
    /// Issued once, since evicted to stay within the store's bounds.
    Expired,
    /// Never issued.
    Unknown,
}

impl LookupError {
    /// Stable code for an error catalogue entry.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed_handle",
            Self::Expired => "evidence_expired",
            Self::Unknown => "unknown_handle",
        }
    }

    /// Sentence for the agent that asked.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::Malformed => "Not an evidence handle. Expected <scheme>://<kind>/<id>.",
            Self::Expired => {
                "That evidence has been dropped to bound memory. Re-run the check to \
                 produce it again; the reply's own summary is still accurate."
            }
            Self::Unknown => "No evidence was ever stored under that handle.",
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    entries: VecDeque<Entry>,
    bytes: usize,
    /// Highest id ever issued. An id at or below this that is not present was
    /// evicted; one above it never existed.
    high_water: u64,
}

/// Bounded, in-memory store of artefacts addressed by URI.
#[derive(Debug)]
pub struct EvidenceStore {
    inner: Mutex<Inner>,
    scheme: String,
    capacity: usize,
    max_bytes: usize,
}

impl Default for EvidenceStore {
    fn default() -> Self {
        Self::new("kicad")
    }
}

impl EvidenceStore {
    /// A store issuing URIs under `scheme`, with the default bounds.
    #[must_use]
    pub fn new(scheme: impl Into<String>) -> Self {
        Self::with_limits(scheme, DEFAULT_CAPACITY, DEFAULT_MAX_BYTES)
    }

    /// A store with explicit bounds. Both are floors of one, because a store
    /// that can hold nothing would turn every handle into an expired one.
    #[must_use]
    pub fn with_limits(scheme: impl Into<String>, capacity: usize, max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            scheme: scheme.into(),
            capacity: capacity.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    /// The URI scheme this store issues.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Store `body` under `kind` and return the handle that fetches it.
    ///
    /// An oversized body is stored anyway and then evicts everything else,
    /// including itself if it alone exceeds the budget — in which case the
    /// handle resolves as [`LookupError::Expired`], which is the truth.
    pub fn put(&self, kind: &str, summary: impl Into<String>, body: Value) -> Handle {
        let bytes = serde_json::to_string(&body).map_or(0, |s| s.len());
        let mut inner = self.lock();
        inner.high_water += 1;
        let id = inner.high_water;
        let entry = Entry {
            id,
            kind: kind.to_string(),
            summary: summary.into(),
            body,
            bytes,
        };
        let uri = entry.uri(&self.scheme);
        inner.bytes += bytes;
        inner.entries.push_back(entry);
        while inner.entries.len() > self.capacity || inner.bytes > self.max_bytes {
            let Some(dropped) = inner.entries.pop_front() else {
                break;
            };
            inner.bytes = inner.bytes.saturating_sub(dropped.bytes);
        }
        Handle { uri, id }
    }

    /// Resolve a URI issued by this store.
    ///
    /// # Errors
    ///
    /// [`LookupError::Malformed`] for a URI of another shape or scheme,
    /// [`LookupError::Expired`] for one that has been evicted, and
    /// [`LookupError::Unknown`] for an id never issued.
    pub fn get(&self, uri: &str) -> Result<Entry, LookupError> {
        let (kind, id) = self.parse(uri)?;
        let inner = self.lock();
        if let Some(entry) = inner
            .entries
            .iter()
            .find(|e| e.id == id && e.kind == kind)
            .cloned()
        {
            return Ok(entry);
        }
        // An id inside the issued range whose entry is gone was evicted. One
        // outside it, or one whose kind does not match, was never issued: the
        // caller has invented a handle, and saying "expired" would imply the
        // server once produced it.
        if id <= inner.high_water && inner.entries.iter().all(|e| e.id != id) {
            Err(LookupError::Expired)
        } else {
            Err(LookupError::Unknown)
        }
    }

    /// Every retained entry, newest first.
    #[must_use]
    pub fn list(&self) -> Vec<Entry> {
        let inner = self.lock();
        inner.entries.iter().rev().cloned().collect()
    }

    /// How many entries are retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether anything has been stored and kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn parse<'a>(&self, uri: &'a str) -> Result<(&'a str, u64), LookupError> {
        let rest = uri
            .strip_prefix(&self.scheme)
            .and_then(|r| r.strip_prefix("://"))
            .ok_or(LookupError::Malformed)?;
        let (kind, id) = rest.rsplit_once('/').ok_or(LookupError::Malformed)?;
        if kind.is_empty() {
            return Err(LookupError::Malformed);
        }
        let id: u64 = id.parse().map_err(|_| LookupError::Malformed)?;
        Ok((kind, id))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_handle_round_trips() {
        let store = EvidenceStore::new("kicad");
        let handle = store.put("diff", "symbol +2", json!({"changes": ["R1 added"]}));
        assert_eq!(handle.uri, "kicad://diff/1");
        let entry = store.get(&handle.uri).unwrap();
        assert_eq!(entry.summary, "symbol +2");
        assert_eq!(entry.body["changes"][0], "R1 added");
    }

    #[test]
    fn ids_are_unique_across_kinds() {
        let store = EvidenceStore::new("kicad");
        let a = store.put("diff", "", json!({}));
        let b = store.put("evidence", "", json!({}));
        assert_eq!(a.uri, "kicad://diff/1");
        assert_eq!(b.uri, "kicad://evidence/2");
    }

    #[test]
    fn an_id_of_the_wrong_kind_is_not_the_same_handle() {
        let store = EvidenceStore::new("kicad");
        let handle = store.put("diff", "", json!({}));
        assert_eq!(handle.uri, "kicad://diff/1");
        assert_eq!(store.get("kicad://evidence/1"), Err(LookupError::Unknown));
    }

    #[test]
    fn an_evicted_handle_says_so_rather_than_denying_it_existed() {
        let store = EvidenceStore::with_limits("kicad", 2, DEFAULT_MAX_BYTES);
        let first = store.put("diff", "", json!({}));
        store.put("diff", "", json!({}));
        store.put("diff", "", json!({}));
        assert_eq!(store.get(&first.uri), Err(LookupError::Expired));
        assert_eq!(store.get("kicad://diff/99"), Err(LookupError::Unknown));
    }

    #[test]
    fn the_byte_budget_evicts_before_the_count_does() {
        let store = EvidenceStore::with_limits("kicad", 64, 200);
        let first = store.put("diff", "", json!({ "pad": "x".repeat(150) }));
        let second = store.put("diff", "", json!({ "pad": "y".repeat(150) }));
        assert_eq!(store.get(&first.uri), Err(LookupError::Expired));
        assert!(store.get(&second.uri).is_ok());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn a_uri_of_another_shape_is_malformed_not_unknown() {
        let store = EvidenceStore::new("kicad");
        for uri in [
            "http://diff/1",
            "kicad://diff",
            "kicad://diff/abc",
            "kicad:///1",
            "",
        ] {
            assert_eq!(store.get(uri), Err(LookupError::Malformed), "{uri}");
        }
    }

    #[test]
    fn listing_is_newest_first() {
        let store = EvidenceStore::new("kicad");
        store.put("diff", "one", json!({}));
        store.put("diff", "two", json!({}));
        let listed = store.list();
        assert_eq!(listed[0].summary, "two");
        assert_eq!(listed[1].summary, "one");
    }
}
