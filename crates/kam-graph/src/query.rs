//! The shape of a question, and the shape of an answer that cannot be a
//! document dump.
//!
//! [`Query`] is deliberately a plain, serializable filter rather than a
//! trait or a closure: the caller asking it is, in practice, an MCP layer
//! that received JSON over the wire and has no Rust type to hand back. Every
//! field is optional and combines with every other by intersection — "kind
//! `wire` on document `main` within this region" — because a caller that
//! wants everything of one kind should not have to also know every document
//! name.
//!
//! [`QueryResult`] never carries the whole match: `items` is capped at
//! [`Graph`](crate::graph::Graph)'s hard limit regardless of what the caller
//! asked for, and `total` says how much was left out. A result that could
//! not tell the difference between "here is everything" and "here is the
//! first 20 of 4,000" would make every query answer with prose an agent has
//! to double-check.

use kam_evidence::Attr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An axis-aligned box in the same units as the indexed points. Bounds are
/// inclusive so a region built from a bounding box that touches an item does
/// not silently exclude it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// Lower bound, horizontal.
    pub min_x: f64,
    /// Lower bound, vertical.
    pub min_y: f64,
    /// Upper bound, horizontal.
    pub max_x: f64,
    /// Upper bound, vertical.
    pub max_y: f64,
}

impl Region {
    /// A region from explicit bounds.
    #[must_use]
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub(crate) fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

/// A combinable filter over a [`Graph`](crate::graph::Graph).
///
/// Every field narrows the result; an unset field imposes no constraint.
/// `attrs` is a list rather than a map because a caller may filter on the
/// same attribute name twice in principle, and because JSON round-trips a
/// list of pairs without needing string keys reserved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Query {
    /// Restrict to items of this kind.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<String>,
    /// Restrict to items sharing this label. Labels are not unique — the
    /// same reference designator can appear on more than one document — so
    /// this can still match several items.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
    /// Restrict to one document.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub document: Option<String>,
    /// Restrict to items whose attribute equals this value, for every pair
    /// listed. Only attributes named at
    /// [`GraphBuilder::index_attr`](crate::graph::GraphBuilder::index_attr)
    /// can be filtered this way; asking about any other attribute is a
    /// [`QueryError::AttributeNotIndexed`], not an empty result.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attrs: Vec<(String, Attr)>,
    /// Restrict to items whose indexed point falls within this region.
    /// Requires a spatial index to have been built at all; see
    /// [`QueryError::NoSpatialIndex`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub region: Option<Region>,
    /// How many matches to return. Defaults to a small number and is
    /// clamped to a hard cap regardless of what is asked for — see the
    /// module docs on why a query result cannot be a document dump.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<usize>,
}

impl Query {
    /// An unfiltered query — matches every item, subject to the default
    /// limit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict to this kind.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Restrict to this label.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Restrict to this document.
    #[must_use]
    pub fn document(mut self, document: impl Into<String>) -> Self {
        self.document = Some(document.into());
        self
    }

    /// Add an attribute-value constraint. Multiple calls AND together.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: Attr) -> Self {
        self.attrs.push((name.into(), value));
        self
    }

    /// Restrict to this region.
    #[must_use]
    pub fn region(mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        self.region = Some(Region::new(min_x, min_y, max_x, max_y));
        self
    }

    /// Cap the number of matches returned.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Why a query could not be answered.
///
/// Both variants are refusals to lie by omission: silently treating an
/// unindexed attribute as "no match" would make a filter that was typo'd or
/// never wired up look like a legitimate empty result, and the same goes for
/// a region query against a graph nobody told to index points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The query filtered on an attribute name that was never passed to
    /// [`GraphBuilder::index_attr`](crate::graph::GraphBuilder::index_attr).
    AttributeNotIndexed(String),
    /// The query used a spatial filter (`region`, or [`neighbors`] on a
    /// point) but the graph was built without
    /// [`GraphBuilder::index_points`](crate::graph::GraphBuilder::index_points).
    ///
    /// [`neighbors`]: crate::graph::Graph::neighbors
    NoSpatialIndex,
    /// [`neighbors`](crate::graph::Graph::neighbors) was asked about an item
    /// that does not exist in the graph.
    ItemNotFound,
    /// [`neighbors`](crate::graph::Graph::neighbors) was asked about an item
    /// that exists but carries no indexed point.
    ItemHasNoPoint,
}

impl QueryError {
    /// Stable code for an error catalogue entry.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::AttributeNotIndexed(_) => "attribute_not_indexed",
            Self::NoSpatialIndex => "no_spatial_index",
            Self::ItemNotFound => "item_not_found",
            Self::ItemHasNoPoint => "item_has_no_point",
        }
    }

    /// Sentence for the agent that asked.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::AttributeNotIndexed(name) => format!(
                "Attribute `{name}` is not indexed. Only attributes passed to \
                 GraphBuilder::index_attr can be filtered on; a query against \
                 any other attribute would silently miss every match."
            ),
            Self::NoSpatialIndex => "No spatial index was built for this graph. Pass an attribute \
                 name to GraphBuilder::index_points to enable region and \
                 neighbor queries."
                .to_string(),
            Self::ItemNotFound => "No item exists under that document, kind and key.".to_string(),
            Self::ItemHasNoPoint => "That item exists but carries no indexed point, so it has no \
                 neighbors to report."
                .to_string(),
        }
    }
}

/// One item as it appears in a [`QueryResult`].
///
/// `document` is omitted when the query already filtered to a single
/// document — repeating it on every item would be the same fact stated as
/// many times as there are matches. `attrs` is omitted when empty for the
/// same reason: a compact result is one that does not spend tokens saying
/// nothing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultItem {
    /// The document this item belongs to, unless the query already pinned
    /// it to one document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// What sort of thing this is.
    pub kind: String,
    /// Stable identity within its kind and document.
    pub key: String,
    /// What to call it in a one-line summary.
    pub label: String,
    /// Distance from the query point, set only by
    /// [`neighbors`](crate::graph::Graph::neighbors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// The item's attributes.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub attrs: BTreeMap<String, Attr>,
}

/// The answer to a truncatable query.
///
/// `total` is the real match count; `items` is capped. The two are reported
/// separately on purpose — see the module docs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryResult {
    /// How many items actually matched, before truncation.
    pub total: usize,
    /// The matches actually returned, at most the effective limit.
    pub items: Vec<ResultItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_includes_its_own_boundary() {
        let region = Region::new(0.0, 0.0, 10.0, 10.0);
        assert!(region.contains(0.0, 0.0));
        assert!(region.contains(10.0, 10.0));
        assert!(!region.contains(10.1, 5.0));
    }

    #[test]
    fn a_query_builder_chains_into_the_same_filter_a_caller_would_send_as_json() {
        let query = Query::new()
            .kind("symbol")
            .label("U1")
            .document("main")
            .attr("net", Attr::text("GND"))
            .region(0.0, 0.0, 10.0, 10.0)
            .limit(5);
        assert_eq!(query.kind.as_deref(), Some("symbol"));
        assert_eq!(query.attrs.len(), 1);
        assert_eq!(query.limit, Some(5));
    }

    #[test]
    fn an_unset_query_serializes_with_no_empty_fields() {
        let query = Query::new();
        let json = serde_json::to_value(&query).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }
}
