//! Structured tool-call error taxonomy.
//!
//! MCP's `CallToolResult` spec only has `content` + `is_error`, with no top-level
//! `data` field — so structured errors ride inside the text content as JSON:
//!
//! ```json
//! {
//!   "message": "Tool 'place_component' is in toolset 'pcb_components' …",
//!   "error": {
//!     "kind": "toolset_not_loaded",
//!     "toolset": "pcb_components",
//!     "tool": "place_component"
//!   }
//! }
//! ```
//!
//! Clients that want to branch on error type parse the body and match on
//! `kind`. Plain clients just render the `message` field as text. The observer
//! extracts `kind` from structured errors so the JSONL log uses a stable
//! vocabulary regardless of which handler produced the error.
//!
//! Every error also carries a `transient` class, which answers the only
//! question a recovery loop actually has: *is retrying this the same call worth
//! anything?* A bare `retryable: bool` cannot express the interesting case —
//! `state` means the world moved, so reconcile first and retry blind never
//! works, while `lock` means wait and try the identical call again.
//!
//! ## Adding a new kind
//!
//! 1. Add a variant to `ToolErrorKind`. Keep field names `snake_case` — they
//!    serialize directly.
//! 2. Add a match arm in `short_code()` and in `transient_class()`.
//! 3. Use `CallToolResult::error_kind(ToolErrorKind::Foo {...}, "message")` in
//!    the handler that produces it.
//! 4. Prefer structured errors for anything the LLM might want to react to
//!    differently (retry, prompt for input). Leave free-text
//!    `CallToolResult::error()` for truly one-off messages.

use super::protocol::{CallToolResult, ToolContent};
use serde::Serialize;

/// Whether retrying the *same* call can succeed, and what has to happen first.
///
/// Deliberately not a boolean. The two cases a boolean collapses are the two a
/// caller most needs apart: `Lock` says "the identical call will work shortly",
/// `State` says "the identical call will fail forever until you re-read the
/// world". Conflating them produces retry storms on one and give-ups on the
/// other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransientClass {
    /// Deterministic. The same call fails the same way; fix the request.
    None,
    /// The network or an external service refused; the same call may work.
    Network,
    /// Something took too long. The call may have partially applied — check
    /// before replaying, or replay with an `operation_id`.
    Timeout,
    /// A resource is held (KiCAD busy, document locked). Wait, retry as is.
    Lock,
    /// The world changed underneath the call. Re-read state and recompute;
    /// a blind retry is useless.
    State,
}

impl TransientClass {
    /// Suggested wait before a retry, for the classes where waiting helps.
    #[must_use]
    pub fn retry_after_ms(self) -> Option<u64> {
        match self {
            Self::Lock => Some(250),
            Self::Network | Self::Timeout => Some(1_000),
            Self::None | Self::State => None,
        }
    }
}

/// Structured error for tool call failures.
///
/// Serializes with `kind` as a stable discriminant — the single field a client
/// or observer needs to match on.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolErrorKind {
    /// The tool exists in the registry but its toolset isn't loaded.
    /// Client recovers in one hop: `load_toolset(toolset)` then retry.
    ToolsetNotLoaded { toolset: String, tool: String },
    /// No tool with this name exists in any registered toolset.
    UnknownTool { tool: String },
    /// A required argument is missing or malformed.
    InvalidArgument { field: String, reason: String },
    /// KiCAD exposes no API for this capability, so the step has to be taken
    /// by hand in the GUI (plan.md D.6.3). `step` is never prose written at
    /// the call site: it comes from the capability's own
    /// [`crate::capability::Limitation::GuiOnlyNoApi`] reason, the same string
    /// `docs/capability-matrix.md` renders, so the error and the matrix cannot
    /// drift. Not transient in any sense a retry helps with — the API does not
    /// exist, and the only way forward is a human in the editor.
    ManualStepRequired { tool: String, step: String },
    /// The process is running under an [`kam_state::OperatingMode`] that
    /// refuses this tool's [`crate::capability::Effect`] (plan.md D.8). A
    /// retry of the identical call can never help — the mode is fixed for
    /// the life of the process (D69) — so the only recovery is a different,
    /// read-only call, or restarting the server under a less restrictive
    /// `KONNECT_MODE`.
    WriteRefusedByMode { tool: String, mode: String },
    /// A referenced file doesn't exist or can't be read.
    FileNotFound { path: String },
    /// An address (document, kind, key) resolves to no item — a `graph_*`
    /// tool's `document`/`kind`/`key` named nothing in the index. Distinct
    /// from `FileNotFound`: the project resolved and built, the address
    /// inside it did not.
    NotFound {
        document: String,
        item_kind: String,
        key: String,
    },
    /// A document is not at the revision the caller's plan was built against.
    /// Re-read it, recompute, retry — never overwrite.
    StaleRevision {
        path: String,
        expected: String,
        actual: String,
    },
    /// The same `operation_id` is already executing. Retrying now risks a
    /// double-apply; wait for the first call to answer.
    ///
    /// # Scope: one process, and that is deliberate (L.2.4)
    ///
    /// This is only reachable over the HTTP transport. `run_stdio`
    /// (`konnect/src/transport/stdio.rs`) reads one JSON-RPC line and awaits it
    /// to completion before reading the next, so a single stdio process can
    /// never have two `kicad_invoke` calls in flight; `axum::serve` does serve
    /// requests concurrently over one shared `ToolContext`, which is where two
    /// claims on one key can actually meet.
    ///
    /// Two processes never race here either: `kam_state::IdempotencyLedger` is
    /// in-memory per `ToolContext`, so a second process starts with an empty
    /// ledger and sees any key as `Fresh`. That is the intended design, not an
    /// oversight — the window it protects is a client retrying a call it just
    /// made, measured in seconds, and making it survive a restart would mean a
    /// journal on disk whose staleness is its own failure mode.
    ///
    /// The cross-process case is covered by a *different* mechanism, and a
    /// reader looking for it here should look there instead: `base_revisions`
    /// refuses a batch built against a document another writer has since
    /// changed ([`Self::StaleRevision`]), and the per-write compare-and-swap
    /// refuses one that changes mid-batch ([`Self::Conflict`]). Neither is
    /// keyed on the caller's identity, so neither cares which process — or
    /// which GUI — moved the document.
    OperationInFlight { operation_id: String },
    /// A write's compare-and-swap found the file had changed since it was
    /// read — a concurrent writer (another Konnect process, or a GUI like
    /// KiCad itself that does not honor the advisory lock) saved in between.
    /// Distinct from `StaleRevision`: this is caught mid-batch by the exact
    /// content comparison in `write_atomic_if_unchanged`, not by the
    /// `base_revisions` check done once before the batch starts. Re-read the
    /// document and recompute — the same edit reapplied blindly will race
    /// again.
    Conflict { path: String },
    /// A document was found and read, and its structure does not support the
    /// operation: a section the format requires is absent, or a block is there
    /// but unusable.
    ///
    /// D77 — added once six sites across four files had converged on the same
    /// shape, not for any one of them. It is the gap between four kinds that
    /// each nearly fit and none of which is true: [`Self::Io`] (the read
    /// succeeded), [`Self::FileNotFound`] (the file is there),
    /// [`Self::InvalidArgument`] (the call is well-formed — the caller named a
    /// document that exists), and [`Self::NotFound`] (the addressed item is
    /// present; it is the document around it that cannot be used).
    ///
    /// What a caller does with it is what makes it worth a kind: nothing about
    /// the *call* can change, so retrying and re-parameterising are both
    /// wrong. The document has to be repaired or replaced, which is why
    /// `detail` says what was missing rather than only that something was.
    MalformedDocument { path: String, detail: String },
    /// A third-party service this tool depends on did not produce a usable
    /// answer — the JLCPCB archive host, a datasheet provider.
    ///
    /// D78 — the seven download and lookup sites of `tools/integration.rs` had
    /// no kind that was true of them: nothing here is the caller's fault, the
    /// filesystem's, or KiCAD's. `code` carries the distinction a recovery
    /// loop needs, because "the host is down" and "the host answered with
    /// something that is not a chunk count" are the same sentence to a reader
    /// and opposite instructions to a client:
    ///
    /// * `"unreachable"` — the request never completed (DNS, TLS, timeout).
    /// * `"server_error"` — HTTP 5xx or 429; the service is having a bad time.
    /// * `"client_error"` — HTTP 4xx; the URL or the request is wrong, and
    ///   replaying it changes nothing.
    /// * `"unexpected_response"` — the service answered, and the body is not
    ///   what the protocol says it should be. Retrying is pointless; either
    ///   the service changed or the wrong URL was configured.
    ///
    /// `service` is the host or product, never a full URL with credentials in
    /// it; the URL belongs in `detail`, which a human reads.
    UpstreamFailed {
        service: String,
        code: &'static str,
        detail: String,
    },
    /// A filesystem failure, with a stable code independent of the OS locale.
    ///
    /// `detail` keeps the operating system's own message — useful to a human,
    /// useless to match on: the same failure reads "The system cannot find the
    /// file specified" or "Le fichier spécifié est introuvable" depending on
    /// the machine. `code` is what a recovery loop branches on.
    Io { code: &'static str, detail: String },
    /// The KiCAD IPC transport never delivered the request, so nothing can
    /// have been applied. `code` is what a recovery loop branches on:
    /// `"unreachable"` (a socket path exists, nothing answered on it) is
    /// fixed by starting KiCAD and replaying the identical call;
    /// `"not_configured"` (no socket path at all) is fixed only by
    /// configuration, never by a retry. Classified from
    /// [`konnect_ipc::IpcFailure`], never from message text.
    IpcUnavailable { code: &'static str, detail: String },
    /// A live KiCAD received the request over IPC and refused it. Distinct
    /// from `IpcUnavailable` in the way that matters: the call reached the
    /// editor, so the request — not the environment — is what has to change,
    /// and a file edit behind that editor could be silently overwritten.
    IpcRejected { detail: String },
    /// A live KiCAD answered, but the board the call names is not among its
    /// open documents. `open` is what KiCAD actually holds, so the caller can
    /// name the fix (open `requested`, or address one of `open`) instead of
    /// guessing. Empty `open` means no PCB document is open at all.
    BoardNotOpen {
        requested: String,
        open: Vec<String>,
    },
    /// Catch-all for handler `anyhow::Error` that hasn't been migrated yet.
    /// Eventually each variant above subsumes a subset of these.
    HandlerError {
        reason: String,
        /// Plausible `lib_id`s / library names for an unresolved KiCAD
        /// symbol lookup — a deterministic did-you-mean, never a silent
        /// substitution (#lib_id candidates). Empty for every other
        /// `HandlerError`, so the field is absent from their JSON.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        candidates: Vec<String>,
    },
    /// A plan's own `validators` promise did not hold after it ran.
    ///
    /// This is what makes `apply_plan` an honest "done": a plan that declared
    /// `erc_clean` and left an ERC error is not finished, no matter how many of
    /// its steps succeeded. Returned as an error specifically so the enclosing
    /// `kicad_invoke` batch (D4) rolls the whole plan back — a caller must never
    /// be left holding the half of a promise that broke it.
    PostconditionFailed {
        /// The validator name as declared in the plan, e.g. `erc_clean`.
        check: String,
        /// The document the postcondition was about.
        document: String,
        /// Errors present after the plan ran.
        errors: usize,
        /// Warnings present after the plan ran.
        warnings: usize,
        /// Finding ids the delta form found new, or the clean form found still
        /// present at error severity. Empty when the validator could not run at
        /// all (see `reason`).
        introduced: Vec<String>,
        /// Set when the validator itself could not run — a missing `kicad-cli`
        /// must surface as a failed postcondition, never as zero findings (E4).
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl ToolErrorKind {
    /// Short, stable string identifier. Matches the serialized `kind` field.
    /// Used by the observer so JSONL logs carry a canonical vocabulary no
    /// matter where the error originated.
    pub fn short_code(&self) -> &'static str {
        match self {
            Self::ToolsetNotLoaded { .. } => "toolset_not_loaded",
            Self::UnknownTool { .. } => "unknown_tool",
            Self::InvalidArgument { .. } => "invalid_argument",
            Self::ManualStepRequired { .. } => "manual_step_required",
            Self::WriteRefusedByMode { .. } => "write_refused_by_mode",
            Self::FileNotFound { .. } => "file_not_found",
            Self::NotFound { .. } => "not_found",
            Self::StaleRevision { .. } => "stale_revision",
            Self::OperationInFlight { .. } => "operation_in_flight",
            Self::Conflict { .. } => "conflict",
            Self::MalformedDocument { .. } => "malformed_document",
            Self::UpstreamFailed { .. } => "upstream_failed",
            Self::Io { .. } => "io",
            Self::IpcUnavailable { .. } => "ipc_unavailable",
            Self::IpcRejected { .. } => "ipc_rejected",
            Self::BoardNotOpen { .. } => "board_not_open",
            Self::HandlerError { .. } => "handler_error",
            Self::PostconditionFailed { .. } => "postcondition_failed",
        }
    }

    /// Whether retrying this exact call can help.
    pub fn transient_class(&self) -> TransientClass {
        match self {
            // The tool exists and the fix is mechanical, but it is a *different*
            // call (load_toolset) that has to happen first — the world, not the
            // request, is wrong.
            Self::ToolsetNotLoaded { .. } | Self::StaleRevision { .. } | Self::Conflict { .. } => {
                TransientClass::State
            }
            Self::OperationInFlight { .. } => TransientClass::Lock,
            Self::UnknownTool { .. }
            | Self::InvalidArgument { .. }
            | Self::FileNotFound { .. }
            | Self::NotFound { .. }
            | Self::ManualStepRequired { .. }
            | Self::WriteRefusedByMode { .. } => TransientClass::None,
            Self::Io { code, .. } => match *code {
                "would_block" | "interrupted" | "resource_busy" => TransientClass::Lock,
                "timed_out" => TransientClass::Timeout,
                "connection_refused"
                | "connection_reset"
                | "connection_aborted"
                | "host_unreachable"
                | "network_unreachable"
                | "not_connected" => TransientClass::Network,
                _ => TransientClass::None,
            },
            // The heart of D.6.5: these three used to be one `String`, and the
            // only kind that took all three claimed `None` — telling a caller
            // not to retry something that starting KiCAD makes work.
            Self::IpcUnavailable { code, .. } => match *code {
                // The transport refused; the same call succeeds once KiCAD is
                // listening. Nothing about the request has to change.
                "unreachable" => TransientClass::Network,
                // No socket to dial: there is nothing to wait for, and a retry
                // loop here is exactly the false `transient` D75 forbids.
                _ => TransientClass::None,
            },
            // Deterministic: KiCAD received the request and refused it. The
            // identical call gets the identical refusal.
            Self::IpcRejected { .. } => TransientClass::None,
            // Only the two codes where waiting is the recovery get a class
            // that promises it. A 4xx and a body that is not what the protocol
            // says are both deterministic, and a retry loop on either is a
            // client burning its budget on an answer that will not change.
            Self::UpstreamFailed { code, .. } => match *code {
                "unreachable" | "server_error" => TransientClass::Network,
                _ => TransientClass::None,
            },
            // The document on disk is what has to change, and no wait makes
            // that happen. Not `State` either: `State` promises that
            // reconciling and retrying is the recovery, and re-reading a file
            // whose (layers) section is missing returns the same file.
            Self::MalformedDocument { .. } => TransientClass::None,
            // The world is not where the caller believed. Open the right board
            // (or address the one that is open) — replaying blind never helps.
            Self::BoardNotOpen { .. } => TransientClass::State,
            // Unclassified by construction. Claiming a class for an error we
            // have not looked at is worse than admitting we do not know.
            Self::HandlerError { .. } => TransientClass::None,
            // Deterministic: the same plan broke the same promise. A retry
            // needs a different plan, not a second attempt at this one.
            Self::PostconditionFailed { .. } => TransientClass::None,
        }
    }

    /// Classify a handler's `anyhow::Error`.
    ///
    /// Walks the source chain for a `std::io::Error` so the agent-facing result
    /// carries a stable `io` code instead of only the OS's localized prose
    /// (progress.md E9). Anything unrecognised stays `HandlerError` rather than
    /// being given a class it has not earned.
    pub fn from_anyhow(err: &anyhow::Error) -> Self {
        for cause in err.chain() {
            // Checked before the io::Error case below: a write conflict
            // carries no io::Error at all — write_atomic_if_unchanged
            // returns it as soon as the re-read content differs, not as an
            // OS failure — so preserving the type here is the only way to
            // classify it as `state` rather than losing it to `HandlerError`.
            if let Some(konnect_schematic_editor::error::Error::Conflict(path)) =
                cause.downcast_ref::<konnect_schematic_editor::error::Error>()
            {
                return Self::Conflict {
                    path: path.display().to_string(),
                };
            }
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                return Self::from_io(io);
            }
        }
        Self::HandlerError {
            reason: err.to_string(),
            candidates: Vec::new(),
        }
    }

    /// Classify a `std::io::Error` directly, for the callers that already hold
    /// one and would otherwise have to wrap it in an `anyhow::Error` just to
    /// get the same answer out of [`Self::from_anyhow`] — which delegates
    /// here, so the two can never disagree on the code.
    pub fn from_io(error: &std::io::Error) -> Self {
        Self::Io {
            code: io_code(error.kind()),
            detail: error.to_string(),
        }
    }
}

/// Stable, locale-independent name for a `std::io::ErrorKind`.
///
/// `ErrorKind` is `#[non_exhaustive]` and its `Debug` text is not a stability
/// promise, so the mapping is explicit for the kinds that actually occur here
/// and falls back to `other` rather than to a string that could change.
fn io_code(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind as K;
    match kind {
        K::NotFound => "not_found",
        K::PermissionDenied => "permission_denied",
        K::AlreadyExists => "already_exists",
        K::WouldBlock => "would_block",
        // `transient_class` has always listed "resource_busy" under `Lock`, but
        // nothing produced the code, so the class was unreachable. It is what
        // `konnect-sexp`'s `refine_rename_failure` raises when a rename is
        // blocked by another application's open handle rather than by an ACL
        // (L.2.5) — the one `permission_denied` that is genuinely worth waiting
        // out.
        K::ResourceBusy => "resource_busy",
        K::Interrupted => "interrupted",
        K::TimedOut => "timed_out",
        K::InvalidData => "invalid_data",
        K::InvalidInput => "invalid_input",
        K::UnexpectedEof => "unexpected_eof",
        K::WriteZero => "write_zero",
        K::ConnectionRefused => "connection_refused",
        K::ConnectionReset => "connection_reset",
        K::ConnectionAborted => "connection_aborted",
        K::NotConnected => "not_connected",
        K::BrokenPipe => "broken_pipe",
        K::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}

impl CallToolResult {
    /// Build a structured error result: JSON body with `message` + `error: {...}`.
    /// Preferred over `CallToolResult::error(text)` whenever the error has a
    /// stable kind the client / LLM might want to branch on.
    pub fn error_kind(kind: ToolErrorKind, message: impl Into<String>) -> Self {
        let transient = kind.transient_class();
        let mut error = serde_json::to_value(&kind)
            .unwrap_or_else(|_| serde_json::json!({ "kind": kind.short_code() }));
        // `transient` and `retry_after_ms` live beside `kind` rather than in a
        // nested object: a recovery loop reads all three together, and errors
        // are the one payload where a byte spent on structure is a byte the
        // model does not spend on the message.
        if let Some(map) = error.as_object_mut() {
            map.insert("transient".to_string(), serde_json::json!(transient));
            if let Some(ms) = transient.retry_after_ms() {
                map.insert("retry_after_ms".to_string(), serde_json::json!(ms));
            }
        }
        let body = serde_json::json!({
            "message": message.into(),
            "error": error,
        });
        CallToolResult {
            content: vec![ToolContent::Text {
                text: body.to_string(),
            }],
            is_error: true,
        }
    }
}

/// Extract the `kind` discriminant from a structured error result, if any.
///
/// Returns `None` for success results. Returns `Some("handler_error")` as a
/// fallback for legacy plain-text errors so the observer's JSONL column is
/// always populated.
pub fn extract_error_kind(result: &CallToolResult) -> Option<String> {
    if !result.is_error {
        return None;
    }
    for c in &result.content {
        if let ToolContent::Text { text } = c {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(k) = v
                    .get("error")
                    .and_then(|e| e.get("kind"))
                    .and_then(|k| k.as_str())
                {
                    return Some(k.to_string());
                }
            }
        }
    }
    // Legacy plain-text error — known-unknown category.
    Some("handler_error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_round_trips_through_extract() {
        let r = CallToolResult::error_kind(
            ToolErrorKind::ToolsetNotLoaded {
                toolset: "pcb_board".into(),
                tool: "add_zone".into(),
            },
            "Toolset not loaded.",
        );
        assert!(r.is_error);
        assert_eq!(
            extract_error_kind(&r).as_deref(),
            Some("toolset_not_loaded")
        );
    }

    #[test]
    fn plain_text_error_extracts_as_handler_error() {
        let r = CallToolResult::error("something blew up");
        assert_eq!(extract_error_kind(&r).as_deref(), Some("handler_error"));
    }

    #[test]
    fn success_result_extracts_as_none() {
        let r = CallToolResult::text("ok");
        assert_eq!(extract_error_kind(&r), None);
    }

    #[test]
    fn short_code_matches_serialized_kind_field() {
        // If these ever drift, clients that match on the `kind` string will
        // silently break. Pin them here.
        let kinds = [
            ToolErrorKind::ToolsetNotLoaded {
                toolset: "x".into(),
                tool: "y".into(),
            },
            ToolErrorKind::UnknownTool { tool: "x".into() },
            ToolErrorKind::InvalidArgument {
                field: "f".into(),
                reason: "r".into(),
            },
            ToolErrorKind::ManualStepRequired {
                // Deliberately not a real MANIFEST tool name: the coverage
                // scanner reads test sources for tool names, and a fixture
                // here would credit that tool with a proof it does not have
                // (D45 — a tool is never proved by its own error).
                tool: "a_gui_only_tool".into(),
                step: "do it in the editor".into(),
            },
            ToolErrorKind::WriteRefusedByMode {
                tool: "add_component".into(),
                mode: "read-only".into(),
            },
            ToolErrorKind::FileNotFound { path: "p".into() },
            ToolErrorKind::NotFound {
                document: "a.kicad_sch".into(),
                item_kind: "symbol".into(),
                key: "ghost".into(),
            },
            ToolErrorKind::StaleRevision {
                path: "p".into(),
                expected: "a".into(),
                actual: "b".into(),
            },
            ToolErrorKind::OperationInFlight {
                operation_id: "op_1".into(),
            },
            ToolErrorKind::Conflict {
                path: "a.kicad_sch".into(),
            },
            ToolErrorKind::Io {
                code: "not_found",
                detail: "d".into(),
            },
            ToolErrorKind::MalformedDocument {
                path: "a.kicad_sch".into(),
                detail: "no (lib_id …) in the symbol block".into(),
            },
            ToolErrorKind::UpstreamFailed {
                service: "example.invalid".into(),
                code: "server_error",
                detail: "HTTP 503".into(),
            },
            ToolErrorKind::IpcUnavailable {
                code: "unreachable",
                detail: "d".into(),
            },
            ToolErrorKind::IpcRejected { detail: "d".into() },
            ToolErrorKind::BoardNotOpen {
                requested: "a.kicad_pcb".into(),
                open: vec!["b.kicad_pcb".into()],
            },
            ToolErrorKind::HandlerError {
                reason: "r".into(),
                candidates: vec![],
            },
            ToolErrorKind::PostconditionFailed {
                check: "erc".into(),
                document: "a.kicad_sch".into(),
                errors: 1,
                warnings: 0,
                introduced: vec!["abc123".into()],
                reason: None,
            },
        ];
        for kind in kinds {
            let code = kind.short_code();
            let json = serde_json::to_value(&kind).unwrap();
            let serialized_kind = json.get("kind").and_then(|v| v.as_str()).unwrap();
            assert_eq!(code, serialized_kind, "drift for {:?}", kind);
        }
    }

    #[test]
    fn errors_carry_a_transient_class_and_a_retry_hint_where_waiting_helps() {
        let stale = CallToolResult::error_kind(
            ToolErrorKind::StaleRevision {
                path: "a.kicad_sch".into(),
                expected: "aaaa-10".into(),
                actual: "bbbb-12".into(),
            },
            "Document changed.",
        );
        let body = error_body(&stale);
        assert_eq!(body["error"]["transient"], "state");
        assert!(
            body["error"].get("retry_after_ms").is_none(),
            "a blind retry never fixes a stale revision, so do not invite one"
        );

        let busy = CallToolResult::error_kind(
            ToolErrorKind::OperationInFlight {
                operation_id: "op_9".into(),
            },
            "Already running.",
        );
        let body = error_body(&busy);
        assert_eq!(body["error"]["transient"], "lock");
        assert_eq!(body["error"]["retry_after_ms"], 250);
    }

    #[test]
    fn structured_fields_survive_the_transient_annotation() {
        // The annotation is injected into the serialized kind; it must not
        // displace the fields a caller reads to recover.
        let r = CallToolResult::error_kind(
            ToolErrorKind::StaleRevision {
                path: "a.kicad_sch".into(),
                expected: "aaaa-10".into(),
                actual: "bbbb-12".into(),
            },
            "Document changed.",
        );
        let body = error_body(&r);
        assert_eq!(body["error"]["kind"], "stale_revision");
        assert_eq!(body["error"]["path"], "a.kicad_sch");
        assert_eq!(body["error"]["expected"], "aaaa-10");
        assert_eq!(extract_error_kind(&r).as_deref(), Some("stale_revision"));
    }

    #[test]
    fn io_failures_get_a_stable_code_and_keep_the_localized_text_as_detail() {
        // progress.md E9: the OS message is French on this machine and English
        // on CI. Matching on it is a bug; matching on `code` is not.
        let io = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Le fichier spécifié est introuvable. (os error 2)",
        );
        let err = anyhow::Error::new(io).context("Failed to read schematic");
        let kind = ToolErrorKind::from_anyhow(&err);
        match &kind {
            ToolErrorKind::Io { code, detail } => {
                assert_eq!(*code, "not_found");
                assert!(detail.contains("introuvable"));
            }
            other => panic!("expected an io kind, got {other:?}"),
        }
        assert_eq!(kind.transient_class(), TransientClass::None);
    }

    #[test]
    fn a_non_io_handler_error_is_not_given_a_class_it_has_not_earned() {
        let err = anyhow::anyhow!("symbol library 'Device' not found");
        let kind = ToolErrorKind::from_anyhow(&err);
        assert_eq!(kind.short_code(), "handler_error");
        assert_eq!(kind.transient_class(), TransientClass::None);
    }

    #[test]
    fn a_write_conflict_is_classified_as_state_not_lost_to_handler_error() {
        // write_atomic_if_unchanged raises this the moment its re-read content
        // differs from what was expected — no io::Error involved — so this is
        // the case from_anyhow's io-only chain walk used to miss entirely,
        // falling back to HandlerError/None: a client told "deterministic, fix
        // your request" for the one error that means "re-read and recompute".
        let path = std::path::PathBuf::from("board.kicad_sch");
        let inner = konnect_schematic_editor::error::Error::Conflict(path.clone());
        let err = anyhow::Error::new(inner).context("Failed to write schematic");
        let kind = ToolErrorKind::from_anyhow(&err);
        match &kind {
            ToolErrorKind::Conflict { path: p } => assert_eq!(p, "board.kicad_sch"),
            other => panic!("expected a conflict kind, got {other:?}"),
        }
        assert_eq!(kind.transient_class(), TransientClass::State);
        assert_eq!(kind.transient_class().retry_after_ms(), None);
    }

    #[test]
    fn a_locked_file_is_classified_as_worth_retrying() {
        let io = std::io::Error::new(std::io::ErrorKind::WouldBlock, "locked");
        let kind = ToolErrorKind::from_anyhow(&anyhow::Error::new(io));
        assert_eq!(kind.transient_class(), TransientClass::Lock);
        assert_eq!(kind.transient_class().retry_after_ms(), Some(250));
    }

    /// L.2.5 — the two halves of `ERROR_ACCESS_DENIED`, once `konnect-sexp`
    /// has told them apart, must land on opposite sides of the taxonomy. The
    /// `Lock` side was dead code until now: `transient_class` listed
    /// `"resource_busy"` but `io_code` never produced it.
    #[test]
    fn a_busy_file_is_worth_waiting_for_and_a_denied_one_never_is() {
        let busy = std::io::Error::new(std::io::ErrorKind::ResourceBusy, "held open");
        let busy = ToolErrorKind::from_anyhow(&anyhow::Error::new(busy));
        assert!(
            matches!(
                &busy,
                ToolErrorKind::Io {
                    code: "resource_busy",
                    ..
                }
            ),
            "a busy file must reach the caller under its own code: {busy:?}"
        );
        assert_eq!(busy.transient_class(), TransientClass::Lock);
        assert_eq!(busy.transient_class().retry_after_ms(), Some(250));

        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "acl");
        let denied = ToolErrorKind::from_anyhow(&anyhow::Error::new(denied));
        assert!(
            matches!(
                &denied,
                ToolErrorKind::Io {
                    code: "permission_denied",
                    ..
                }
            ),
            "{denied:?}"
        );
        assert_eq!(
            denied.transient_class(),
            TransientClass::None,
            "waiting out an ACL denial is a hang, not a recovery"
        );
        assert_eq!(denied.transient_class().retry_after_ms(), None);
    }

    fn error_body(result: &CallToolResult) -> serde_json::Value {
        let ToolContent::Text { text } = &result.content[0] else {
            panic!("structured errors are text content")
        };
        serde_json::from_str(text).expect("structured error body is JSON")
    }
}
