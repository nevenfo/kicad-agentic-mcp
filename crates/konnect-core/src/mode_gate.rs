//! The one check every tool-execution entry point runs before a handler
//! sees the call: does the process's [`kam_state::OperatingMode`] allow this
//! tool's [`capability::Effect`] and [`capability::WriteTarget`]
//! (plan.md D.8, D.8.3)?
//!
//! There are exactly two call sites, both named in `plan.md`'s validation
//! for D.8: [`mcp::handler::McpHandler::dispatch_tool`] (direct tool calls
//! and every meta-tool except `kicad_invoke`) and
//! [`router::meta_tools::handle_kicad_invoke`] (one check per batch entry,
//! since the gateway's own effect label is a coverage-audit artefact, not a
//! bound on what its entries may do — see the comment on
//! `capability::META_TOOL_EFFECTS`'s `kicad_invoke` entry). Both call this
//! module before invoking a handler, never after (INV4): a refusal here
//! means the handler never runs, so there is nothing to roll back.

use crate::capability::{self, Effect, WriteTarget};
use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tools::ToolContext;

/// `Some(kind)` when `tool`'s `effect`/`write_target` must be refused under
/// `ctx`'s current mode; `None` when the call may proceed. `write_target` is
/// only consulted when `effect` is [`Effect::Write`] — pass
/// [`WriteTarget::DesignDocument`] for a `Read` tool, it changes nothing.
/// Never consults the tool's arguments or its handler — the decision is
/// made from the mode, the effect and the write target alone, before either
/// handler or arguments matter.
pub fn check(
    ctx: &ToolContext,
    tool: &str,
    effect: Effect,
    write_target: WriteTarget,
) -> Option<ToolErrorKind> {
    let mode = ctx.mode.current();
    if capability::mode_allows(mode, effect, write_target) {
        None
    } else {
        Some(ToolErrorKind::WriteRefusedByMode {
            tool: tool.to_string(),
            mode: mode.to_string(),
        })
    }
}

/// A ready-to-return `CallToolResult` refusal, or `None` when the call is
/// allowed. For the direct-call and meta-tool paths in `dispatch_tool`,
/// which return one result per call. `kicad_invoke`'s batch loop uses
/// [`check`] directly instead, because it needs the refusal as one entry
/// among many rather than as the whole result.
///
/// The message names *why* — refused because the mode is read-only versus
/// refused because the design is frozen for manufacturing are different
/// facts, and `mode` alone (also carried on the returned [`ToolErrorKind`])
/// lets a caller tell them apart programmatically; the message spells it
/// out for a human or a model reading the text.
pub fn refuse(
    ctx: &ToolContext,
    tool: &str,
    effect: Effect,
    write_target: WriteTarget,
) -> Option<CallToolResult> {
    let kind = check(ctx, tool, effect, write_target)?;
    let mode = ctx.mode.current();
    let message = match mode {
        kam_state::OperatingMode::Manufacturing => format!(
            "Tool '{tool}' can modify a source design document, and the server is running in \
             operating mode '{mode}' — the design is frozen for manufacturing: reads, checks \
             and fabrication outputs are allowed, but no source document may change. Nothing \
             ran. Restart the server under 'write' or 'experimental' to allow it."
        ),
        _ => format!(
            "Tool '{tool}' can write, and the server is running in operating mode '{mode}', \
             which refuses every write. Nothing ran. Restart the server under a less \
             restrictive KONNECT_MODE to allow it."
        ),
    };
    Some(CallToolResult::error_kind(kind, message))
}
