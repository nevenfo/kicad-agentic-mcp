//! A generic, queryable index over item sets — the "world model" an agent
//! queries instead of the document itself.
//!
//! [`kam_evidence::ItemSet`] is a flat bag of items for one document version;
//! it answers "what is here" but not "every symbol on net GND" or "what is
//! near this pad" without a linear scan the caller repeats on every turn.
//! [`Graph`] builds a handful of `BTreeMap` indices once, at construction,
//! and every query intersects them instead of re-scanning.
//!
//! `BTreeMap` specifically, not `HashMap`: an agent re-issues similar queries
//! turn after turn, and a client caching by request prefix needs the same
//! query to answer in the same order every time. A `HashMap`'s iteration
//! order is an implementation detail that would turn that cache into a
//! source of flaky replies.
//!
//! The one rule every query obeys: it can never hand back more than it says
//! it found. A document can hold thousands of items; an agent's context
//! cannot. Every truncatable query reports `total` — what actually
//! matched — separately from the `items` it returns, capped well short of
//! what a reply can carry. A caller that wants more asks a narrower
//! question; this crate does not offer a way to raise the cap past it.
//!
//! Format-agnostic, like `kam-evidence`: this crate knows kinds, labels,
//! keys, attributes and documents, never a file format. Whoever extracts an
//! [`ItemSet`] decides what an attribute means and which ones are worth
//! indexing; this crate indexes only what it is told to, and says so
//! explicitly — rather than silently returning nothing — when asked to
//! filter on an attribute nobody asked it to index.
//!
//! What this crate deliberately does not do: persist anything, cache across
//! revisions, or infer connectivity. A [`Graph`] is immutable once built and
//! knows only the attributes it was handed; deciding what a net is belongs to
//! whoever owns the file format, not to the index.

#![deny(missing_docs)]

pub mod graph;
pub mod query;

pub use graph::{Graph, GraphBuilder, Stats};
pub use query::{Query, QueryError, QueryResult, Region, ResultItem};

pub use kam_evidence::{Attr, Item, ItemSet};
