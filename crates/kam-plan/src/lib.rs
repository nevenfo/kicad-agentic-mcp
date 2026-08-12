//! A plan is a change described once, checked before it runs, and executed
//! without asking anyone what to do next.
//!
//! The shape this exists to replace is a model in a loop: call a tool, read the
//! result, decide the next call, call it, read it, decide again. Every arrow in
//! that loop is an inference, and inference is the most expensive and least
//! reliable step in the system. A plan collapses the loop into two moves —
//! *write the plan*, then *run it* — by giving the caller the two things the
//! loop was really being used for:
//!
//! * **[`compile`] expands** — one operation may become many tool calls, so the
//!   caller states an intent (`decouple this IC`) instead of enumerating and,
//!   worse, computing the calls that implement it. Arithmetic done in the
//!   expansion is arithmetic a model cannot get wrong.
//! * **[`refs`] resolves** — a later operation can name an earlier one's output
//!   (`${place.reference}`) rather than round-tripping through the model to
//!   read it back.
//!
//! What makes that safe rather than merely shorter is that both are settled
//! *before* the first mutation. [`compile`] refuses a plan whose references
//! point at an operation that does not exist or has not run yet, so a plan that
//! cannot finish never starts — the same order of business as checking
//! preconditions before claiming an idempotency key.
//!
//! The crate is domain-free by construction. It knows a plan has operations; it
//! does not know what an operation means. Whoever owns the domain implements
//! [`compile::OpLibrary`] to turn an operation into tool calls, and drives
//! [`execute::Execution`] to run them. That keeps this crate clean-room (see
//! `plan.md`, D11) and means a second domain costs a library rather than a
//! second planner.
//!
//! ```
//! use kam_plan::{compile, execute, ir, program};
//! use serde_json::json;
//!
//! struct Echo;
//! impl compile::OpLibrary for Echo {
//!     fn expand(&self, op: &ir::Op) -> Result<Vec<program::StepSpec>, compile::ExpandError> {
//!         Ok(vec![program::StepSpec {
//!             tool: op.op.clone(),
//!             args: op.with.clone(),
//!         }])
//!     }
//! }
//!
//! let plan = ir::Plan::from_json(&json!({
//!     "ops": [
//!         {"id": "a", "op": "make", "with": {"name": "R1"}},
//!         {"op": "use", "with": {"target": "${a.reference}"}}
//!     ]
//! }))
//! .unwrap();
//!
//! let program = compile::compile(&plan, &Echo).unwrap();
//! let mut run = execute::Execution::new(program);
//!
//! // Step one runs and reports what it produced.
//! let execute::Next::Step(step) = run.next_step() else { panic!() };
//! assert_eq!(step.tool, "make");
//! run.record(true, json!({"reference": "R1"}));
//!
//! // Step two was written against that output and never needed a model to
//! // read it back.
//! let execute::Next::Step(step) = run.next_step() else { panic!() };
//! assert_eq!(step.args, json!({"target": "R1"}));
//! ```

#![deny(missing_docs)]

pub mod compile;
pub mod execute;
pub mod ir;
pub mod program;
pub mod refs;

pub use compile::{compile, CompileError, ExpandError, OpLibrary};
pub use execute::{Execution, ExecutionReport, Next, ResolvedStep, StepResult};
pub use ir::{is_valid_op_id, Op, Plan, PlanError, RollbackPolicy, IR_VERSION};
pub use program::{Expansion, Program, Step, StepSpec};
pub use refs::{
    is_valid_ref_target, position_target, rewrite, Outputs, RefError, Reference, RewriteError,
    Segment,
};
