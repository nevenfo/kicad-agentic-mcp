//! Domain-level diffing for design documents.
//!
//! A batch of edits that reports `3 files changed` cannot be reviewed. This
//! crate turns two versions of a document into the sentence a reviewer wanted:
//! `C17 added`, `U4 moved: (84,31) -> (82,29)`, `VDD3V3 connections: +2`.
//!
//! It is format-agnostic on purpose. Whoever owns the file format extracts an
//! [`ItemSet`]; this crate matches items by stable key and reports attribute
//! differences. That keeps it clean-room and re-licensable (see `plan.md`,
//! D11), and it means a second document format costs an extractor rather than
//! a second diff engine.
//!
//! The other half of the same idea is [`store`]: the one-line summary belongs
//! in the reply, the item-by-item detail belongs behind a handle the caller
//! resolves only when it wants to challenge the change.

#![deny(missing_docs)]

pub mod diff;
pub mod model;
pub mod store;

pub use diff::{AttrChange, Change, Diff};
pub use model::{Attr, Item, ItemSet};
pub use store::{Entry, EvidenceStore, Handle, LookupError};
