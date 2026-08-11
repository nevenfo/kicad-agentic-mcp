//! Turning KiCAD documents into the vocabulary `kam-evidence` diffs.
//!
//! `kam-evidence` knows how to say "U4 moved" but nothing about KiCAD. This
//! module is the other half: it reads a `.kicad_sch` or `.kicad_pcb` and emits
//! an [`ItemSet`] whose keys are KiCAD's own UUIDs, so an item that was
//! re-serialised, reordered or reformatted is still recognised as the same
//! item.
//!
//! [`validators`] is the other half of an evidence pack: the diff says what
//! moved, `kicad-cli` says whether the design still holds.
//!
//! Extraction is deliberately lossy. A diff that reports every S-expression
//! node is the textual diff we are replacing; what a reviewer needs is which
//! symbols, tracks, zones and nets changed, and how.

mod pcb;
mod schematic;
pub mod validators;

use kam_evidence::{Diff, ItemSet};
use std::path::Path;

/// Which extractor, if any, applies to a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// `.kicad_sch`
    Schematic,
    /// `.kicad_pcb`
    Pcb,
}

impl DocumentKind {
    /// Classify by extension. `None` means "no domain extractor" — the caller
    /// falls back to reporting the file as changed without saying how, which is
    /// honest, rather than guessing.
    #[must_use]
    pub fn of_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("kicad_sch") => Some(Self::Schematic),
            Some("kicad_pcb") => Some(Self::Pcb),
            _ => None,
        }
    }
}

/// Extract every diffable item from a document's bytes.
///
/// A document that does not parse yields `None` rather than an empty set:
/// empty means "this file has no items", which would report a broken file as a
/// wholesale deletion. Refusing to describe it is the safe answer.
#[must_use]
pub fn extract(kind: DocumentKind, bytes: &[u8]) -> Option<ItemSet> {
    let text = std::str::from_utf8(bytes).ok()?;
    let root = konnect_sexp::parse_sexp(text).ok()?;
    match kind {
        DocumentKind::Schematic => Some(schematic::extract(&root)),
        DocumentKind::Pcb => Some(pcb::extract(&root)),
    }
}

/// What changed between two versions of one document.
///
/// `before` is `None` for a file that did not exist, which is how a newly
/// created sheet reports every one of its items as added.
#[must_use]
pub fn diff_document(path: &Path, before: Option<&[u8]>, after: Option<&[u8]>) -> Option<Diff> {
    let kind = DocumentKind::of_path(path)?;
    let before = match before {
        Some(bytes) => extract(kind, bytes)?,
        None => ItemSet::new(),
    };
    let after = match after {
        Some(bytes) => extract(kind, bytes)?,
        None => ItemSet::new(),
    };
    Some(Diff::compute(&before, &after))
}

/// Shared S-expression helpers. KiCAD 9 and 10 write `(uuid "…")`; older
/// documents write `(tstamp …)`. Both are the same identity for our purposes.
pub(crate) mod sexp {
    use konnect_sexp::SexpNode;

    /// The node's own identity, if it has one.
    pub(crate) fn uuid(node: &SexpNode) -> Option<String> {
        node.find("uuid")
            .or_else(|| node.find("tstamp"))
            .and_then(|n| n.get(1))
            .and_then(|n| n.as_str())
            .map(str::to_string)
    }

    /// A `(at x y [angle])` position.
    pub(crate) fn at(node: &SexpNode) -> Option<(f64, f64)> {
        let at = node.find("at")?;
        Some((at.get_f64(1)?, at.get_f64(2)?))
    }

    /// The rotation from `(at x y angle)`, absent when the node has no angle.
    pub(crate) fn angle(node: &SexpNode) -> Option<f64> {
        node.find("at")?.get_f64(3)
    }

    /// A `(property "Name" "Value" …)` lookup. Used for references, values and
    /// footprints in both document types.
    pub(crate) fn property<'a>(node: &'a SexpNode, name: &str) -> Option<&'a str> {
        node.find_all("property")
            .into_iter()
            .find(|p| p.get(1).and_then(|n| n.as_str()) == Some(name))
            .and_then(|p| p.get(2))
            .and_then(|n| n.as_str())
    }

    /// A fallback key for items KiCAD wrote without a UUID: their position and
    /// ordinal. Weaker than a UUID — an unnamed item that moves reads as one
    /// removal plus one addition — but better than dropping it from the diff.
    pub(crate) fn positional_key(prefix: &str, node: &SexpNode, ordinal: usize) -> String {
        match at(node) {
            Some((x, y)) => format!("{prefix}@{x:.4},{y:.4}"),
            None => format!("{prefix}#{ordinal}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn only_design_documents_have_an_extractor() {
        assert_eq!(
            DocumentKind::of_path(&PathBuf::from("a.kicad_sch")),
            Some(DocumentKind::Schematic)
        );
        assert_eq!(
            DocumentKind::of_path(&PathBuf::from("a.kicad_pcb")),
            Some(DocumentKind::Pcb)
        );
        assert_eq!(DocumentKind::of_path(&PathBuf::from("a.kicad_pro")), None);
    }

    #[test]
    fn a_document_that_does_not_parse_is_not_described() {
        assert!(extract(DocumentKind::Schematic, b"(kicad_sch unterminated").is_none());
    }

    #[test]
    fn a_file_with_no_extractor_yields_no_diff_rather_than_a_wrong_one() {
        assert!(diff_document(&PathBuf::from("a.kicad_pro"), Some(b"{}"), Some(b"{ }")).is_none());
    }

    #[test]
    fn a_created_document_reports_its_items_as_added() {
        let after = br#"(kicad_sch (junction (at 10 20) (uuid "j-1")))"#;
        let diff = diff_document(&PathBuf::from("a.kicad_sch"), None, Some(after)).unwrap();
        assert_eq!(diff.summary(), "junction +1");
    }
}
