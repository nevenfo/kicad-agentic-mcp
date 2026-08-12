//! The KiCAD half of the plan IR: what an operation *means*.
//!
//! `kam-plan` knows a plan has operations and that one may refer to another's
//! output. It does not know what any of them do — that is here, in a single
//! [`OpLibrary`] implementation, so the planner stays clean-room and a second
//! document format would cost a library rather than a second planner
//! (plan.md, D11).
//!
//! ## What an operation is for
//!
//! Two things, and only two:
//!
//! * **It does arithmetic a model should not be asked to do.** Every coordinate
//!   an operation emits is snapped to the 1.27 mm schematic grid before it
//!   leaves. That is not a nicety: E6 in `progress.md` is a schematic that
//!   passed every tool call and failed ERC six times because a power symbol was
//!   written at the coordinate it was given, 0.33 mm off the pin it was meant
//!   to touch. Inside a plan that class of error is unreachable.
//! * **It collapses a sequence into an intent.** `decouple` is one operation and
//!   three tool calls per capacitor. The caller states which pins want
//!   decoupling; it does not enumerate placements, connections and ground
//!   symbols, and — more to the point — does not re-derive them through an
//!   inference per call.
//!
//! ## What an operation is not for
//!
//! Judging the design. `decouple` places capacitors where it is told and wires
//! them mechanically; it does not know whether 100 nF is the right value for
//! that rail, and it does not say so. Whether the result holds is answered by
//! `kicad-cli` through `kicad_invoke(verify: "auto")`, which is the only source
//! in this system entitled to say a design is sound (E7).

mod ops;

pub use ops::{description as plan_description, op_names, KicadOps, OP_LIBRARY};
