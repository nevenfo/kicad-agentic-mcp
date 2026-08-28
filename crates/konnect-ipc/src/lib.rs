#[allow(clippy::all, warnings)]
pub mod gen;

pub mod builders;
pub mod client;
pub mod transform;
pub mod types;

pub use client::{
    BoardNotOpen, IpcFailure, KiCadIpcClient, OpenDocument, SaveDocumentOutcome,
    TransportUnreachable,
};
pub use types::*;
