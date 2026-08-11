//! The index itself: built once from whatever documents are handed to the
//! builder, immutable after that.
//!
//! Immutability is the whole design. Nothing here recomputes an index on
//! read, so a query is always an intersection of `BTreeMap` lookups rather
//! than a scan — and nothing here has to reconcile a mutation with an index
//! that has already been handed out to a caller mid-query. Whoever owns
//! revisions (the KiCAD-facing half of this project, not this crate) decides
//! when a new [`Graph`] gets built; this crate only decides how fast it
//! answers once it exists.
//!
//! Every item is addressed by `(document, kind, key)`, because `kind, key`
//! alone (the identity [`kam_evidence::Item`] already uses) is only unique
//! within one document — a [`Graph`] deliberately spans several.

use crate::query::{Query, QueryError, QueryResult, ResultItem};
use kam_evidence::{Attr, Item, ItemSet};
use std::collections::{BTreeMap, BTreeSet};

/// How a query answers when the caller did not ask for a specific size.
/// Small enough that an agent orienting itself with an unfiltered query
/// gets a sample, not a wall of text.
const DEFAULT_LIMIT: usize = 20;

/// The most a single query can ever return, no matter what `limit` asks for.
/// This is the actual guarantee this crate makes — see the crate docs — so
/// it is enforced here rather than trusted to callers.
const HARD_LIMIT: usize = 200;

/// An item's full address within a [`Graph`]: document, kind, key.
type Triple = (String, String, String);

/// Narrow a candidate set by intersecting with another. `None` means "no
/// filter applied yet", which is why this is not simply `BTreeSet::default`
/// intersected — an empty starting set would make the *first* filter applied
/// always produce zero results.
fn narrow(candidate: Option<BTreeSet<Triple>>, set: BTreeSet<Triple>) -> BTreeSet<Triple> {
    match candidate {
        None => set,
        Some(prev) => prev.intersection(&set).cloned().collect(),
    }
}

fn clamp_limit(limit: usize) -> usize {
    limit.min(HARD_LIMIT)
}

/// Counts a caller uses to decide what to look at next, without asking for
/// any item at all. This is the cheapest possible query against a
/// [`Graph`] — a summary line, not a sample of items.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Item count per document.
    pub documents: BTreeMap<String, usize>,
    /// Item count per kind, across every document.
    pub kinds: BTreeMap<String, usize>,
}

/// Builds a [`Graph`] from one or more documents' worth of items.
///
/// Indexing every attribute of every item would waste memory on attributes
/// nobody queries — a coordinate stored once per item and again in an
/// attribute-value index is pure overhead if nothing ever filters on exact
/// coordinate equality. So the builder is told, explicitly, which attributes
/// are worth an index; anything else stays queryable only by
/// [`Graph::get`].
#[derive(Debug, Clone, Default)]
pub struct GraphBuilder {
    documents: Vec<(String, ItemSet)>,
    indexed_attrs: BTreeSet<String>,
    spatial_attr: Option<String>,
}

impl GraphBuilder {
    /// Add one document's items under `name`. Calling this twice with the
    /// same name is not rejected — the later set's items simply take the
    /// address of any earlier item that shares a `(kind, key)`, the same
    /// last-write-wins rule [`ItemSet::insert`] already uses within a
    /// document.
    #[must_use]
    pub fn document(mut self, name: impl Into<String>, set: ItemSet) -> Self {
        self.documents.push((name.into(), set));
        self
    }

    /// Build a value index for this attribute name, so [`Query::attr`] can
    /// filter on it. Attributes not named here are still stored on every
    /// [`Item`] and visible in results; they are just not filterable, and
    /// asking to filter on one is [`QueryError::AttributeNotIndexed`] rather
    /// than a silent no-op.
    #[must_use]
    pub fn index_attr(mut self, name: impl Into<String>) -> Self {
        self.indexed_attrs.insert(name.into());
        self
    }

    /// Build the spatial index from whichever attribute holds each item's
    /// position. Not hard-coded to a fixed name: the caller extracted the
    /// document and knows what it called the position attribute (`at`, by
    /// convention, but this crate does not assume it). Items whose value
    /// under this key is not an [`Attr::Point`] are simply not spatially
    /// indexed — they still appear in `kind`, `label` and attribute queries.
    #[must_use]
    pub fn index_points(mut self, attr: impl Into<String>) -> Self {
        self.spatial_attr = Some(attr.into());
        self
    }

    /// Consume the builder and produce an immutable [`Graph`].
    #[must_use]
    pub fn build(self) -> Graph {
        let mut items = BTreeMap::new();
        let mut by_kind: BTreeMap<String, BTreeSet<Triple>> = BTreeMap::new();
        let mut by_label: BTreeMap<String, BTreeSet<Triple>> = BTreeMap::new();
        let mut by_attr: BTreeMap<String, BTreeMap<String, BTreeSet<Triple>>> = self
            .indexed_attrs
            .iter()
            .map(|name| (name.clone(), BTreeMap::new()))
            .collect();
        let mut points: BTreeMap<Triple, (f64, f64)> = BTreeMap::new();

        for (document, set) in &self.documents {
            for item in set.iter() {
                let triple: Triple = (document.clone(), item.kind.clone(), item.key.clone());

                by_kind
                    .entry(item.kind.clone())
                    .or_default()
                    .insert(triple.clone());
                by_label
                    .entry(item.label.clone())
                    .or_default()
                    .insert(triple.clone());

                for (name, values) in &mut by_attr {
                    if let Some(value) = item.attrs.get(name) {
                        values
                            .entry(value.to_string())
                            .or_default()
                            .insert(triple.clone());
                    }
                }

                if let Some(spatial) = &self.spatial_attr {
                    if let Some(Attr::Point { x, y }) = item.attrs.get(spatial) {
                        points.insert(triple.clone(), (*x, *y));
                    }
                }

                items.insert(triple, item.clone());
            }
        }

        Graph {
            items,
            by_kind,
            by_label,
            by_attr,
            points,
            spatial_attr: self.spatial_attr,
        }
    }
}

/// An immutable index over items from one or more documents.
///
/// Build with [`Graph::builder`]. There is no mutator on purpose — see the
/// module docs.
#[derive(Debug, Clone)]
pub struct Graph {
    items: BTreeMap<Triple, Item>,
    by_kind: BTreeMap<String, BTreeSet<Triple>>,
    by_label: BTreeMap<String, BTreeSet<Triple>>,
    by_attr: BTreeMap<String, BTreeMap<String, BTreeSet<Triple>>>,
    points: BTreeMap<Triple, (f64, f64)>,
    spatial_attr: Option<String>,
}

impl Graph {
    /// Start building a graph.
    #[must_use]
    pub fn builder() -> GraphBuilder {
        GraphBuilder::default()
    }

    /// Look up exactly one item. Not truncatable — an exact address either
    /// resolves to one item or it does not, so there is nothing here for a
    /// `total`/`items` split to protect against.
    #[must_use]
    pub fn get(&self, document: &str, kind: &str, key: &str) -> Option<&Item> {
        self.items
            .get(&(document.to_string(), kind.to_string(), key.to_string()))
    }

    /// Run a combinable filter, honouring the query's limit but never past
    /// the hard cap.
    ///
    /// # Errors
    ///
    /// [`QueryError::AttributeNotIndexed`] if `query` filters on an
    /// attribute [`GraphBuilder::index_attr`] was never told about, and
    /// [`QueryError::NoSpatialIndex`] if it filters by `region` on a graph
    /// with no spatial index. Both are refusals to silently return an empty
    /// result for a filter that was never wired up — see [`QueryError`].
    pub fn query(&self, query: &Query) -> Result<QueryResult, QueryError> {
        let mut candidate: Option<BTreeSet<Triple>> = None;

        if let Some(kind) = &query.kind {
            let set = self.by_kind.get(kind).cloned().unwrap_or_default();
            candidate = Some(narrow(candidate, set));
        }

        if let Some(label) = &query.label {
            let set = self.by_label.get(label).cloned().unwrap_or_default();
            candidate = Some(narrow(candidate, set));
        }

        for (name, value) in &query.attrs {
            let by_value = self
                .by_attr
                .get(name)
                .ok_or_else(|| QueryError::AttributeNotIndexed(name.clone()))?;
            let set = by_value
                .get(&value.to_string())
                .cloned()
                .unwrap_or_default();
            candidate = Some(narrow(candidate, set));
        }

        if let Some(region) = &query.region {
            if self.spatial_attr.is_none() {
                return Err(QueryError::NoSpatialIndex);
            }
            let set: BTreeSet<Triple> = self
                .points
                .iter()
                .filter(|(_, (x, y))| region.contains(*x, *y))
                .map(|(triple, _)| triple.clone())
                .collect();
            candidate = Some(narrow(candidate, set));
        }

        let mut matches: Vec<Triple> = match candidate {
            Some(set) => set.into_iter().collect(),
            None => self.items.keys().cloned().collect(),
        };

        if let Some(document) = &query.document {
            matches.retain(|triple| &triple.0 == document);
        }

        let total = matches.len();
        let limit = clamp_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
        let omit_document = query.document.is_some();
        let items = matches
            .into_iter()
            .take(limit)
            .map(|triple| self.result_item(&triple, omit_document, None))
            .collect();

        Ok(QueryResult { total, items })
    }

    /// Items whose indexed point is within `radius` of the target item's
    /// point, nearest first. Ties at equal distance break on `(document,
    /// kind, key)` so the order never depends on hash iteration or on which
    /// half of a tie happened to be visited first.
    ///
    /// The search never leaves the target's own document. Two documents index
    /// two unrelated coordinate spaces, so "within 5 units" across them is not
    /// a proximity — it is a coincidence of numbers, and reporting it as
    /// adjacency would be worse than reporting nothing.
    ///
    /// # Errors
    ///
    /// [`QueryError::ItemNotFound`] if the target address does not exist,
    /// [`QueryError::ItemHasNoPoint`] if it exists but was never spatially
    /// indexed (including when the graph has no spatial index at all).
    pub fn neighbors(
        &self,
        document: &str,
        kind: &str,
        key: &str,
        radius: f64,
        limit: usize,
    ) -> Result<QueryResult, QueryError> {
        let target: Triple = (document.to_string(), kind.to_string(), key.to_string());
        if !self.items.contains_key(&target) {
            return Err(QueryError::ItemNotFound);
        }
        let Some(&(tx, ty)) = self.points.get(&target) else {
            return Err(QueryError::ItemHasNoPoint);
        };

        let mut candidates: Vec<(f64, Triple)> = self
            .points
            .iter()
            .filter(|(triple, _)| triple.0 == target.0 && **triple != target)
            .map(|(triple, (x, y))| {
                let distance = ((x - tx).powi(2) + (y - ty).powi(2)).sqrt();
                (distance, triple.clone())
            })
            .filter(|(distance, _)| *distance <= radius)
            .collect();
        candidates.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });

        let total = candidates.len();
        let limit = clamp_limit(limit);
        let items = candidates
            .into_iter()
            .take(limit)
            .map(|(distance, triple)| self.result_item(&triple, false, Some(distance)))
            .collect();

        Ok(QueryResult { total, items })
    }

    /// Cheap orientation: how much is in each document and each kind,
    /// without fetching a single item.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let mut documents = BTreeMap::new();
        let mut kinds = BTreeMap::new();
        for (document, kind, _) in self.items.keys() {
            *documents.entry(document.clone()).or_insert(0) += 1;
            *kinds.entry(kind.clone()).or_insert(0) += 1;
        }
        Stats { documents, kinds }
    }

    fn result_item(
        &self,
        triple: &Triple,
        omit_document: bool,
        distance: Option<f64>,
    ) -> ResultItem {
        let item = &self.items[triple];
        ResultItem {
            document: if omit_document {
                None
            } else {
                Some(triple.0.clone())
            },
            kind: item.kind.clone(),
            key: item.key.clone(),
            label: item.label.clone(),
            distance,
            attrs: item.attrs.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_evidence::Item;

    fn item(kind: &str, key: &str, label: &str) -> Item {
        Item::new(kind, key, label)
    }

    fn build_two_documents() -> Graph {
        let mut main = ItemSet::new();
        main.insert(item("symbol", "u1", "U1").with("net", Attr::text("GND")));
        main.insert(item("symbol", "u2", "U2").with("net", Attr::text("VCC")));
        main.insert(item("wire", "w1", "wire").with("net", Attr::text("GND")));

        let mut power = ItemSet::new();
        power.insert(item("symbol", "u1", "U1").with("net", Attr::text("VCC")));

        Graph::builder()
            .document("main", main)
            .document("power", power)
            .index_attr("net")
            .build()
    }

    #[test]
    fn a_query_truncated_below_the_match_count_reports_the_real_total() {
        let mut set = ItemSet::new();
        for i in 0..30 {
            set.insert(item("symbol", &i.to_string(), "R"));
        }
        let graph = Graph::builder().document("main", set).build();

        let result = graph.query(&Query::new().kind("symbol").limit(5)).unwrap();
        assert_eq!(result.total, 30);
        assert_eq!(result.items.len(), 5);
    }

    #[test]
    fn a_limit_past_the_hard_cap_is_brought_back_to_it() {
        let mut set = ItemSet::new();
        for i in 0..(HARD_LIMIT + 50) {
            set.insert(item("symbol", &i.to_string(), "R"));
        }
        let graph = Graph::builder().document("main", set).build();

        let result = graph
            .query(&Query::new().kind("symbol").limit(HARD_LIMIT + 50))
            .unwrap();
        assert_eq!(result.items.len(), HARD_LIMIT);
        assert_eq!(result.total, HARD_LIMIT + 50);
    }

    #[test]
    fn identical_graphs_answer_the_same_query_in_the_same_order() {
        let build = || {
            let mut set = ItemSet::new();
            set.insert(item("symbol", "b", "U2"));
            set.insert(item("symbol", "a", "U1"));
            set.insert(item("symbol", "c", "U3"));
            Graph::builder().document("main", set).build()
        };
        let first = build().query(&Query::new().kind("symbol")).unwrap();
        let second = build().query(&Query::new().kind("symbol")).unwrap();
        let keys = |r: &QueryResult| r.items.iter().map(|i| i.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&first), keys(&second));
    }

    #[test]
    fn a_document_filter_drops_the_document_field_from_each_result() {
        let graph = build_two_documents();
        let result = graph.query(&Query::new().document("main")).unwrap();
        assert!(result.items.iter().all(|i| i.document.is_none()));

        let result = graph.query(&Query::new().kind("symbol")).unwrap();
        assert!(result.items.iter().all(|i| i.document.is_some()));
    }

    #[test]
    fn an_unindexed_attribute_is_an_explicit_error_not_an_empty_result() {
        let graph = build_two_documents();
        let err = graph.query(&Query::new().attr("footprint", Attr::text("x")));
        assert_eq!(
            err,
            Err(QueryError::AttributeNotIndexed("footprint".to_string()))
        );
    }

    #[test]
    fn an_indexed_attribute_matches_the_same_value_across_documents() {
        let graph = build_two_documents();
        let result = graph
            .query(&Query::new().attr("net", Attr::text("VCC")))
            .unwrap();
        assert_eq!(result.total, 2);
    }

    #[test]
    fn neighbors_excludes_the_target_itself() {
        let mut set = ItemSet::new();
        set.insert(item("pad", "p1", "P1").with("at", Attr::point(0.0, 0.0)));
        set.insert(item("pad", "p2", "P2").with("at", Attr::point(1.0, 0.0)));
        let graph = Graph::builder()
            .document("main", set)
            .index_points("at")
            .build();

        let result = graph.neighbors("main", "pad", "p1", 5.0, 10).unwrap();
        assert!(result.items.iter().all(|i| i.key != "p1"));
        assert_eq!(result.total, 1);
    }

    #[test]
    fn neighbors_never_crosses_into_another_document() {
        let mut sheet = ItemSet::new();
        sheet.insert(item("symbol", "u1", "U1").with("at", Attr::point(100.0, 80.0)));
        let mut board = ItemSet::new();
        board.insert(item("footprint", "c1", "C1").with("at", Attr::point(100.0, 80.0)));
        let graph = Graph::builder()
            .document("sheet", sheet)
            .document("board", board)
            .index_points("at")
            .build();

        // Same numbers, different coordinate spaces: adjacency here would be a
        // coincidence, not a fact about the design.
        let result = graph.neighbors("sheet", "symbol", "u1", 50.0, 10).unwrap();
        assert_eq!(result.total, 0);
    }

    #[test]
    fn neighbors_sorts_by_distance_then_breaks_ties_on_key() {
        let mut set = ItemSet::new();
        set.insert(item("pad", "origin", "O").with("at", Attr::point(0.0, 0.0)));
        set.insert(item("pad", "far", "F").with("at", Attr::point(10.0, 0.0)));
        set.insert(item("pad", "near-b", "B").with("at", Attr::point(1.0, 0.0)));
        set.insert(item("pad", "near-a", "A").with("at", Attr::point(0.0, 1.0)));
        let graph = Graph::builder()
            .document("main", set)
            .index_points("at")
            .build();

        let result = graph.neighbors("main", "pad", "origin", 20.0, 10).unwrap();
        let keys: Vec<_> = result.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["near-a", "near-b", "far"]);
    }

    #[test]
    fn an_item_without_a_point_never_appears_in_a_region_query_or_in_neighbors() {
        let mut set = ItemSet::new();
        set.insert(item("pad", "with-point", "P").with("at", Attr::point(0.0, 0.0)));
        set.insert(item("pad", "no-point", "N").with("net", Attr::text("GND")));
        let graph = Graph::builder()
            .document("main", set)
            .index_points("at")
            .build();

        let region = graph
            .query(&Query::new().region(-100.0, -100.0, 100.0, 100.0))
            .unwrap();
        assert!(region.items.iter().all(|i| i.key != "no-point"));

        let neighbors = graph
            .neighbors("main", "pad", "with-point", 1000.0, 10)
            .unwrap();
        assert!(neighbors.items.iter().all(|i| i.key != "no-point"));
    }

    #[test]
    fn neighbors_on_a_pointless_item_is_an_explicit_error() {
        let mut set = ItemSet::new();
        set.insert(item("pad", "no-point", "N"));
        let graph = Graph::builder()
            .document("main", set)
            .index_points("at")
            .build();
        assert_eq!(
            graph.neighbors("main", "pad", "no-point", 5.0, 10),
            Err(QueryError::ItemHasNoPoint)
        );
    }

    #[test]
    fn neighbors_on_a_missing_item_is_an_explicit_error() {
        let graph = build_two_documents();
        assert_eq!(
            graph.neighbors("main", "symbol", "ghost", 5.0, 10),
            Err(QueryError::ItemNotFound)
        );
    }

    #[test]
    fn a_region_query_without_a_spatial_index_is_an_explicit_error() {
        let graph = build_two_documents();
        assert_eq!(
            graph.query(&Query::new().region(0.0, 0.0, 1.0, 1.0)),
            Err(QueryError::NoSpatialIndex)
        );
    }

    #[test]
    fn stats_counts_by_document_and_by_kind() {
        let graph = build_two_documents();
        let stats = graph.stats();
        assert_eq!(stats.documents["main"], 3);
        assert_eq!(stats.documents["power"], 1);
        assert_eq!(stats.kinds["symbol"], 3);
        assert_eq!(stats.kinds["wire"], 1);
    }

    #[test]
    fn get_finds_an_item_by_its_full_address() {
        let graph = build_two_documents();
        assert_eq!(graph.get("main", "symbol", "u1").unwrap().label, "U1");
        assert!(graph.get("power", "symbol", "u2").is_none());
    }
}
