//! Comparing two [`ItemSet`]s and saying what changed in one line per fact.
//!
//! The output an agent needs is `U4 moved: (84,31) -> (82,29)`, not eleven
//! hunks of S-expression. A textual diff of a KiCAD document is dominated by
//! re-serialisation noise — reordered properties, reformatted floats, new
//! UUIDs — and none of that is a design change. Matching by stable key first
//! and comparing attributes second removes that noise structurally instead of
//! filtering it afterwards.

use crate::model::{trim_float, Attr, Item, ItemSet};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One attribute that is not what it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrChange {
    /// Attribute name, as the extractor spelled it.
    pub name: String,
    /// Value before, absent if the attribute did not exist.
    pub before: Option<Attr>,
    /// Value after, absent if the attribute was dropped.
    pub after: Option<Attr>,
}

/// One thing that happened to one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Change {
    /// The item did not exist before.
    Added {
        /// The item as it now is.
        item: Item,
    },
    /// The item no longer exists.
    Removed {
        /// The item as it was.
        item: Item,
    },
    /// The item exists in both, with different attributes.
    Modified {
        /// Kind, carried so a consumer can group without a second lookup.
        kind: String,
        /// Stable key.
        key: String,
        /// Name after the change — a renamed item is still the same item.
        label: String,
        /// Every attribute that differs, in name order.
        attrs: Vec<AttrChange>,
    },
}

impl Change {
    /// Kind of the item this change is about.
    #[must_use]
    pub fn kind(&self) -> &str {
        match self {
            Self::Added { item } | Self::Removed { item } => &item.kind,
            Self::Modified { kind, .. } => kind,
        }
    }

    /// Label of the item this change is about.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Added { item } | Self::Removed { item } => &item.label,
            Self::Modified { label, .. } => label,
        }
    }

    /// One line, in the vocabulary of the design rather than of the file.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Added { item } => format!("{} {} added", item.kind, item.label),
            Self::Removed { item } => format!("{} {} removed", item.kind, item.label),
            Self::Modified { label, attrs, .. } => {
                let parts: Vec<String> = attrs.iter().map(render_attr).collect();
                format!("{label} {}", parts.join(", "))
            }
        }
    }
}

/// `at (84,31) -> (82,29)` reads better as `moved`, and a scalar that went up
/// by two reads better as `+2` than as `3 -> 5`.
fn render_attr(change: &AttrChange) -> String {
    match (&change.before, &change.after) {
        (Some(Attr::Point { .. }), Some(after @ Attr::Point { .. })) if change.name == "at" => {
            let before = change.before.as_ref().expect("matched Some above");
            format!("moved: {before} -> {after}")
        }
        (Some(Attr::Num(a)), Some(Attr::Num(b))) => {
            let delta = b - a;
            let sign = if delta >= 0.0 { "+" } else { "-" };
            format!("{}: {}{}", change.name, sign, trim_float(delta.abs()))
        }
        (Some(before), Some(after)) => format!("{}: {before} -> {after}", change.name),
        (None, Some(after)) => format!("{}: +{after}", change.name),
        (Some(before), None) => format!("{}: -{before}", change.name),
        (None, None) => change.name.clone(),
    }
}

/// Everything that changed between two versions of a document set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Diff {
    /// Changes in a stable order: kind, then label, then key.
    pub changes: Vec<Change>,
}

impl Diff {
    /// Compare two extracted item sets.
    #[must_use]
    pub fn compute(before: &ItemSet, after: &ItemSet) -> Self {
        let mut changes = Vec::new();

        for item in after.iter() {
            match before.get(&item.kind, &item.key) {
                None => changes.push(Change::Added { item: item.clone() }),
                Some(old) => {
                    let attrs = attr_changes(old, item);
                    if !attrs.is_empty() {
                        changes.push(Change::Modified {
                            kind: item.kind.clone(),
                            key: item.key.clone(),
                            label: item.label.clone(),
                            attrs,
                        });
                    }
                }
            }
        }

        for item in before.iter() {
            if after.get(&item.kind, &item.key).is_none() {
                changes.push(Change::Removed { item: item.clone() });
            }
        }

        changes.sort_by(|a, b| {
            a.kind()
                .cmp(b.kind())
                .then_with(|| a.label().cmp(b.label()))
                .then_with(|| a.render().cmp(&b.render()))
        });

        Self { changes }
    }

    /// Whether the two versions describe the same design.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Number of items touched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Merge another diff into this one, keeping the order stable. Used to fold
    /// per-document diffs into one project-level answer.
    pub fn extend(&mut self, other: Diff) {
        self.changes.extend(other.changes);
        self.changes.sort_by(|a, b| {
            a.kind()
                .cmp(b.kind())
                .then_with(|| a.label().cmp(b.label()))
                .then_with(|| a.render().cmp(&b.render()))
        });
    }

    /// A single line: how many of each kind were added, removed and modified.
    ///
    /// This is what goes in a batch reply. It is bounded by the number of
    /// *kinds*, not by the size of the change, so a thousand-item rewrite still
    /// costs one short line.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.changes.is_empty() {
            return "no design change".to_string();
        }
        let mut tally: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
        for change in &self.changes {
            let entry = tally.entry(change.kind()).or_insert((0, 0, 0));
            match change {
                Change::Added { .. } => entry.0 += 1,
                Change::Removed { .. } => entry.1 += 1,
                Change::Modified { .. } => entry.2 += 1,
            }
        }
        let mut parts = Vec::new();
        for (kind, (added, removed, modified)) in tally {
            let mut bits = Vec::new();
            if added > 0 {
                bits.push(format!("+{added}"));
            }
            if removed > 0 {
                bits.push(format!("-{removed}"));
            }
            if modified > 0 {
                bits.push(format!("~{modified}"));
            }
            parts.push(format!("{kind} {}", bits.join(" ")));
        }
        parts.join(", ")
    }

    /// Up to `max_lines` rendered changes, with a final line saying how many
    /// were withheld.
    ///
    /// A diff that grows without bound is a diff that cannot be put in a reply,
    /// so truncation is part of the contract rather than the caller's problem.
    /// The full list stays available through the evidence handle.
    #[must_use]
    pub fn render_lines(&self, max_lines: usize) -> Vec<String> {
        if self.changes.len() <= max_lines {
            return self.changes.iter().map(Change::render).collect();
        }
        let mut lines: Vec<String> = self
            .changes
            .iter()
            .take(max_lines)
            .map(Change::render)
            .collect();
        lines.push(format!("... {} more", self.changes.len() - max_lines));
        lines
    }

    /// Labels of every item touched, deduplicated — the "which parts of the
    /// board did you touch" question, answered without the detail.
    #[must_use]
    pub fn touched_labels(&self) -> Vec<String> {
        let set: BTreeSet<&str> = self.changes.iter().map(Change::label).collect();
        set.into_iter().map(str::to_string).collect()
    }
}

fn attr_changes(before: &Item, after: &Item) -> Vec<AttrChange> {
    let names: BTreeSet<&String> = before.attrs.keys().chain(after.attrs.keys()).collect();
    let mut out = Vec::new();
    for name in names {
        let old = before.attrs.get(name);
        let new = after.attrs.get(name);
        let same = match (old, new) {
            (Some(a), Some(b)) => a.same_as(b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            out.push(AttrChange {
                name: name.clone(),
                before: old.cloned(),
                after: new.cloned(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(key: &str, label: &str, x: f64, y: f64) -> Item {
        Item::new("symbol", key, label).with("at", Attr::point(x, y))
    }

    #[test]
    fn an_unchanged_document_produces_no_changes() {
        let a: ItemSet = vec![symbol("u1", "U1", 10.0, 20.0)].into_iter().collect();
        let b = a.clone();
        let diff = Diff::compute(&a, &b);
        assert!(diff.is_empty());
        assert_eq!(diff.summary(), "no design change");
    }

    #[test]
    fn a_moved_symbol_reads_as_moved_not_as_two_coordinates() {
        let a: ItemSet = vec![symbol("u4", "U4", 84.0, 31.0)].into_iter().collect();
        let b: ItemSet = vec![symbol("u4", "U4", 82.0, 29.0)].into_iter().collect();
        let diff = Diff::compute(&a, &b);
        assert_eq!(diff.render_lines(10), vec!["U4 moved: (84,31) -> (82,29)"]);
        assert_eq!(diff.summary(), "symbol ~1");
    }

    #[test]
    fn an_added_item_names_its_kind_and_label() {
        let a = ItemSet::new();
        let b: ItemSet = vec![symbol("c17", "C17", 1.0, 2.0)].into_iter().collect();
        let diff = Diff::compute(&a, &b);
        assert_eq!(diff.render_lines(10), vec!["symbol C17 added"]);
        assert_eq!(diff.summary(), "symbol +1");
    }

    #[test]
    fn a_removed_item_is_reported_from_the_before_image() {
        let a: ItemSet = vec![symbol("r9", "R9", 1.0, 2.0)].into_iter().collect();
        let b = ItemSet::new();
        let diff = Diff::compute(&a, &b);
        assert_eq!(diff.render_lines(10), vec!["symbol R9 removed"]);
    }

    #[test]
    fn a_scalar_change_reads_as_a_delta() {
        let a: ItemSet =
            vec![Item::new("net", "vdd", "VDD3V3").with("connections", Attr::Num(3.0))]
                .into_iter()
                .collect();
        let b: ItemSet =
            vec![Item::new("net", "vdd", "VDD3V3").with("connections", Attr::Num(5.0))]
                .into_iter()
                .collect();
        assert_eq!(
            Diff::compute(&a, &b).render_lines(10),
            vec!["VDD3V3 connections: +2"]
        );
    }

    #[test]
    fn a_text_change_shows_both_sides() {
        let a: ItemSet = vec![Item::new("symbol", "r5", "R5").with("value", Attr::text("10k"))]
            .into_iter()
            .collect();
        let b: ItemSet = vec![Item::new("symbol", "r5", "R5").with("value", Attr::text("4k7"))]
            .into_iter()
            .collect();
        assert_eq!(
            Diff::compute(&a, &b).render_lines(10),
            vec!["R5 value: 10k -> 4k7"]
        );
    }

    #[test]
    fn a_renamed_item_is_modified_not_added_and_removed() {
        let a: ItemSet = vec![symbol("uuid-1", "R1", 1.0, 1.0)].into_iter().collect();
        let b: ItemSet = vec![symbol("uuid-1", "R7", 1.0, 1.0).with("ref", Attr::text("R7"))]
            .into_iter()
            .collect();
        let diff = Diff::compute(&a, &b);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff.render_lines(10), vec!["R7 ref: +R7"]);
    }

    #[test]
    fn reformatted_coordinates_are_not_a_change() {
        let a: ItemSet = vec![symbol("u1", "U1", 84.0, 31.0)].into_iter().collect();
        let b: ItemSet = vec![symbol("u1", "U1", 84.000_000_1, 31.0)]
            .into_iter()
            .collect();
        assert!(Diff::compute(&a, &b).is_empty());
    }

    #[test]
    fn rendering_is_bounded_and_says_how_much_it_withheld() {
        let a = ItemSet::new();
        let b: ItemSet = (0..10)
            .map(|i| symbol(&format!("k{i}"), &format!("C{i}"), 0.0, 0.0))
            .collect();
        let lines = Diff::compute(&a, &b).render_lines(3);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[3], "... 7 more");
    }

    #[test]
    fn the_summary_stays_one_short_line_however_big_the_change() {
        let a = ItemSet::new();
        let b: ItemSet = (0..500)
            .map(|i| symbol(&format!("k{i}"), &format!("C{i}"), 0.0, 0.0))
            .collect();
        assert_eq!(Diff::compute(&a, &b).summary(), "symbol +500");
    }

    #[test]
    fn changes_are_ordered_the_same_way_every_run() {
        let a = ItemSet::new();
        let b: ItemSet = vec![
            symbol("z", "Z9", 0.0, 0.0),
            Item::new("wire", "w", "wire"),
            symbol("a", "A1", 0.0, 0.0),
        ]
        .into_iter()
        .collect();
        let first = Diff::compute(&a, &b);
        let second = Diff::compute(&a, &b);
        assert_eq!(first, second);
        assert_eq!(
            first.render_lines(10),
            vec!["symbol A1 added", "symbol Z9 added", "wire wire added"]
        );
    }

    #[test]
    fn touched_labels_answers_what_did_you_touch() {
        let a: ItemSet = vec![symbol("u4", "U4", 1.0, 1.0)].into_iter().collect();
        let b: ItemSet = vec![symbol("u4", "U4", 2.0, 2.0), symbol("c1", "C1", 0.0, 0.0)]
            .into_iter()
            .collect();
        assert_eq!(Diff::compute(&a, &b).touched_labels(), vec!["C1", "U4"]);
    }
}
