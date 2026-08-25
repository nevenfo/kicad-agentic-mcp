//! The single crossing point between a tool handler and KiCAD's IPC API.
//!
//! Four toolsets used to carry a byte-identical `with_ipc` that returned
//! `Result<T, String>`, and that `String` erased the only distinction any
//! caller had to make: a transport that never delivered the request, a live
//! KiCAD that does not hold the named board, and a live KiCAD that received
//! the request and refused it. The first is safe to recover from by editing
//! the board file directly and is fixed by starting KiCAD; the other two are
//! not, because an editor may hold that file open and overwrite the edit on
//! its next save.
//!
//! So the boundary is typed once here ([`with_ipc`]) and catalogued once here
//! ([`ipc_error_result`]), and no handler re-derives either.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tools::ipc_queue;
use konnect_ipc::client::KiCadIpcClient;
use konnect_ipc::IpcFailure;

/// Run `f` against a KiCAD IPC client, serialized (D.9.1: [`ipc_queue`])
/// behind a single worker thread per `addr` so no two calls ever reach KiCAD
/// concurrently, classifying any failure with [`IpcFailure::from_error`] — by
/// the typed marker in the error chain, never by matching the message text.
///
/// Submission into the queue happens synchronously in this function body,
/// before the returned future is polled, so the FIFO order the queue
/// promises is the order callers invoked `with_ipc`, not the order their
/// futures happened to be polled.
///
/// The outer `Err` is reserved for the queue machinery itself (a panicked
/// job, or a worker thread that is no longer running): that is a bug in this
/// process, not a statement about KiCAD, and it must not be mistaken for one.
pub(crate) fn with_ipc<T, F>(
    addr: String,
    f: F,
) -> impl std::future::Future<Output = anyhow::Result<Result<T, IpcFailure>>>
where
    T: Send + 'static,
    F: FnOnce(&KiCadIpcClient) -> anyhow::Result<T> + Send + 'static,
{
    ipc_queue::submit(&addr, move |client: &KiCadIpcClient| {
        f(client).map_err(IpcFailure::from_error)
    })
}

/// The one place an `IpcFailure` becomes an agent-facing error.
///
/// Each variant maps to the `ToolErrorKind` whose `transient_class` is true of
/// it — `network` only where starting KiCAD makes the identical call work,
/// `state` where the board has to be opened first, `none` where KiCAD already
/// answered no. The prose is the prose the call sites already emitted; only
/// the classification is new.
pub(crate) fn ipc_error_result(failure: &IpcFailure) -> CallToolResult {
    ipc_error_result_with(failure, str::to_string)
}

/// As [`ipc_error_result`], but letting the call site rewrite the message —
/// to name the operation, or to say why a file fallback it *has* was withheld.
/// Only the prose is the call site's; the kind and its transient class stay
/// this module's, so no handler can re-classify by accident.
pub(crate) fn ipc_error_result_with(
    failure: &IpcFailure,
    decorate: impl FnOnce(&str) -> String,
) -> CallToolResult {
    match failure {
        IpcFailure::Unconfigured(_) | IpcFailure::Unreachable(_) => {
            let code = if matches!(failure, IpcFailure::Unconfigured(_)) {
                "not_configured"
            } else {
                "unreachable"
            };
            CallToolResult::error_kind(
                ToolErrorKind::IpcUnavailable {
                    code,
                    detail: failure.message().to_string(),
                },
                decorate(&format!(
                    "KiCAD must be running with the board loaded (IPC error: {})",
                    failure.message()
                )),
            )
        }
        IpcFailure::BoardMismatch {
            requested,
            open,
            message,
        } => CallToolResult::error_kind(
            ToolErrorKind::BoardNotOpen {
                requested: requested.clone(),
                open: open.clone(),
            },
            decorate(message),
        ),
        IpcFailure::Rejected(message) => CallToolResult::error_kind(
            ToolErrorKind::IpcRejected {
                detail: message.clone(),
            },
            decorate(&format!("KiCAD rejected the request over IPC: {message}")),
        ),
    }
}

/// The one `ipc!` a handler may reach for: resolves `board` from `$args` and
/// confirms — via [`KiCadIpcClient::ensure_board_is_active`] — that the live
/// KiCAD session actually holds that board before `$body` runs against it.
///
/// P.6.9.22: this used to be two copies, `pcb_components.rs`'s (guarded, this
/// shape) and `pcb_routing.rs`'s own two-argument `ipc!` that skipped the
/// guard and the `board` read entirely, falling through to
/// [`KiCadIpcClient::get_open_documents`]'s first entry
/// (`get_board_document`, private to `konnect-ipc`) — silently routing onto
/// whichever board KiCAD happened to have open first, not the one `board`
/// named. `find_open_board`'s own doc comment records the live symptom: "with
/// the user's own project focused and the target board open behind it,
/// first-document targeting either fails or, worse, would mutate the wrong
/// board." Six of the eight `pcb_routing.rs` handlers that used the unguarded
/// form write copper. One definition now, so a second copy cannot diverge
/// from it silently again — see
/// `required_schema_static_honesty::no_ipc_call_bypasses_the_guarded_macro`
/// for the guard that keeps a future handler from being written next to this
/// path instead of through it.
macro_rules! guarded_ipc {
    ($ctx:expr, $args:expr, |$c:ident| $body:expr) => {{
        let addr = $ctx.config.ipc_address.clone();
        let requested_board = $crate::tools::get_path($args, "board")?;
        match $crate::tools::ipc_boundary::with_ipc(addr, move |$c| {
            $c.ensure_board_is_active(&requested_board)?;
            $body
        })
        .await?
        {
            Ok(v) => v,
            Err(failure) => return Ok($crate::tools::ipc_boundary::ipc_error_result(&failure)),
        }
    }};
}
pub(crate) use guarded_ipc;
