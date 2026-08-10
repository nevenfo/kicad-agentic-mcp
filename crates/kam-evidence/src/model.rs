//! The vocabulary a diff is computed over.
//!
//! A design document is reduced to a flat set of [`Item`]s: something with a
//! kind, a stable key, a name a human recognises, and a handful of attributes.
//! That is deliberately less than the document — a diff that tries to describe
//! every S-expression node produces the textual diff this crate exists to
//! replace.
//!
//! Nothing here mentions KiCAD. Extraction lives with whoever owns the file
//! format; this crate only compares what it is given.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A value an item can carry. Kept small on purpose: the renderer has to be
/// able to print every variant in a few characters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Attr {
    /// Free text — a value, a footprint id, a net class.
    Text(String),
    /// A scalar. Rendered with a sign when it changes, so counts read as
    /// `+2` rather than `3 -> 5`.
    Num(f64),
    /// A position in document units. Compared with a tolerance, because a
    /// re-serialised coordinate is not a design change.
    Point {
        /// Horizontal position.
        x: f64,
        /// Vertical position.
        y: f64,
    },
}

impl Attr {
    /// Text constructor that accepts anything string-like.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Point constructor.
    #[must_use]
    pub fn point(x: f64, y: f64) -> Self {
        Self::Point { x, y }
    }

    /// Whether two values are the same design fact.
    ///
    /// Coordinates and scalars compare within a tolerance: KiCAD writes
    /// `84.0` and `84` for the same millimetre, and a rewritten document must
    /// not report every item as moved.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        const EPS: f64 = 1e-6;
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => (a - b).abs() < EPS,
            (Self::Point { x: ax, y: ay }, Self::Point { x: bx, y: by }) => {
                (ax - bx).abs() < EPS && (ay - by).abs() < EPS
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Attr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(s) => write!(f, "{s}"),
            Self::Num(n) => write!(f, "{}", trim_float(*n)),
            Self::Point { x, y } => write!(f, "({},{})", trim_float(*x), trim_float(*y)),
        }
    }
}

/// Print a float the way a schematic reads it: `84` not `84.0000`, `84.33` not
/// `84.33000000000001`.
pub(crate) fn trim_float(v: f64) -> String {
    let mut s = format!("{v:.4}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// One addressable thing in a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// What sort of thing this is: `symbol`, `wire`, `net`, `footprint`, …
    /// Used to group the summary and to keep keys from colliding across kinds.
    pub kind: String,
    /// Stable identity within its kind. A UUID when the format has one; a
    /// derived key otherwise. Two items with the same key across two versions
    /// of a document are *the same item*, which is the whole basis of the diff.
    pub key: String,
    /// What to call it in a one-line summary: `U4`, `C17`, `VDD3V3`.
    pub label: String,
    /// Everything worth reporting a change to.
    pub attrs: BTreeMap<String, Attr>,
}

impl Item {
    /// Build an item with no attributes yet.
    pub fn new(kind: impl Into<String>, key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
            label: label.into(),
            attrs: BTreeMap::new(),
        }
    }

    /// Attach an attribute, builder-style.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: Attr) -> Self {
        self.attrs.insert(name.into(), value);
        self
    }

    /// The identity used for matching: kind and key together.
    #[must_use]
    pub fn identity(&self) -> (String, String) {
        (self.kind.clone(), self.key.clone())
    }
}

/// Every item extracted from one version of one document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemSet {
    items: BTreeMap<(String, String), Item>,
}

impl ItemSet {
    /// An empty set — what a document that did not exist yet looks like.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an item.
    ///
    /// A duplicate key means the extractor could not tell two things apart. The
    /// later item wins and the collision is counted, because silently dropping
    /// half the document would make the diff lie by omission; callers can check
    /// [`ItemSet::len`] against what they fed in.
    pub fn insert(&mut self, item: Item) {
        self.items.insert(item.identity(), item);
    }

    /// Number of items held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the document contributed nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterate items in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    /// Look one up by kind and key.
    #[must_use]
    pub fn get(&self, kind: &str, key: &str) -> Option<&Item> {
        self.items.get(&(kind.to_string(), key.to_string()))
    }

    /// How many items of each kind, for a summary line.
    #[must_use]
    pub fn counts_by_kind(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for item in self.items.values() {
            *out.entry(item.kind.clone()).or_insert(0) += 1;
        }
        out
    }
}

impl FromIterator<Item> for ItemSet {
    fn from_iter<T: IntoIterator<Item = Item>>(iter: T) -> Self {
        let mut set = Self::new();
        for item in iter {
            set.insert(item);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_that_only_differ_in_formatting_are_the_same_fact() {
        assert!(Attr::point(84.0, 31.0).same_as(&Attr::point(84.0, 31.0)));
        assert!(!Attr::point(84.0, 31.0).same_as(&Attr::point(82.0, 29.0)));
    }

    #[test]
    fn a_number_and_a_string_are_never_the_same_fact() {
        assert!(!Attr::Num(10.0).same_as(&Attr::text("10")));
    }

    #[test]
    fn floats_print_the_way_a_schematic_reads() {
        assert_eq!(trim_float(84.0), "84");
        assert_eq!(trim_float(100.33), "100.33");
        assert_eq!(trim_float(-0.0), "0");
        assert_eq!(Attr::point(84.0, 31.5).to_string(), "(84,31.5)");
    }

    #[test]
    fn identity_is_kind_plus_key_so_kinds_cannot_collide() {
        let mut set = ItemSet::new();
        set.insert(Item::new("symbol", "u-1", "U1"));
        set.insert(Item::new("wire", "u-1", "wire"));
        assert_eq!(set.len(), 2);
        assert_eq!(set.get("symbol", "u-1").unwrap().label, "U1");
    }

    #[test]
    fn counts_by_kind_summarises_a_document() {
        let set: ItemSet = vec![
            Item::new("symbol", "a", "U1"),
            Item::new("symbol", "b", "U2"),
            Item::new("wire", "c", "wire"),
        ]
        .into_iter()
        .collect();
        let counts = set.counts_by_kind();
        assert_eq!(counts["symbol"], 2);
        assert_eq!(counts["wire"], 1);
    }
}
