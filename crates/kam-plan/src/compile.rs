//! Turning a declared plan into runnable steps — and refusing the ones that
//! cannot finish.
//!
//! Compilation does two things, and the second is the one that matters:
//!
//! 1. **Expansion.** Each operation goes to the caller's [`OpLibrary`], which
//!    returns the tool calls that implement it. One operation may become many.
//! 2. **Reference checking.** Every `${op.path}` in every expanded step must
//!    name an operation that appears *strictly earlier* in the plan. A forward
//!    reference, a self-reference, or a reference to an operation that does not
//!    exist is a compile error.
//!
//! Step 2 is what makes a plan safe to start. Without it, a plan can run half
//! way and then discover it cannot continue, leaving a design in a state nobody
//! designed — the failure mode the whole transaction layer exists to prevent.
//! With it, a plan that cannot finish never begins, which costs nothing but a
//! rejected call.
//!
//! Note what is *not* checked: whether the referenced field will actually exist
//! in the output. That depends on what the tool returns at run time, and
//! guessing at it would mean modelling every tool's result shape here. The
//! compiler checks the half it can prove, and [`crate::execute`] reports the
//! other half as a step failure with the reference named.

use crate::ir::{Op, Plan};
use crate::program::{Expansion, Program, Step, StepSpec};
use crate::refs;

/// Turns a declared operation into the tool calls that implement it.
///
/// Implemented by whoever owns the domain. This crate deliberately has no
/// registry of its own: an op library that knew about KiCAD would make this
/// crate know about KiCAD.
pub trait OpLibrary {
    /// Expand one operation.
    ///
    /// # Errors
    ///
    /// Return an [`ExpandError`] for an unknown operation or invalid
    /// parameters. Naming the offending field is what makes the retry targeted
    /// rather than a re-guess.
    fn expand(&self, op: &Op) -> Result<Vec<StepSpec>, ExpandError>;
}

/// Why an operation could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandError {
    /// Stable code, e.g. `unknown_op`, `invalid_argument`.
    pub code: String,
    /// The parameter at fault, when there is one.
    pub field: Option<String>,
    /// Human-readable reason.
    pub reason: String,
}

impl ExpandError {
    /// An operation name the library does not know.
    #[must_use]
    pub fn unknown_op(name: &str) -> Self {
        Self {
            code: "unknown_op".to_string(),
            field: None,
            reason: format!("'{name}' is not a known plan operation"),
        }
    }

    /// A parameter that is missing, malformed, or out of range.
    #[must_use]
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            code: "invalid_argument".to_string(),
            field: Some(field.into()),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{} ({}): {}", self.code, field, self.reason),
            None => write!(f, "{}: {}", self.code, self.reason),
        }
    }
}

/// Why a plan could not be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// An operation's library expansion failed.
    Expansion {
        /// The operation's id.
        op_id: String,
        /// The operation's name.
        op: String,
        /// What the library said.
        error: ExpandError,
    },
    /// A reference in an operation's arguments could not be parsed.
    BadReference {
        /// The operation's id.
        op_id: String,
        /// The parse failure.
        error: refs::RefError,
    },
    /// A reference names an operation that does not exist in the plan.
    UnknownReference {
        /// The operation the reference was written in.
        op_id: String,
        /// The reference as written.
        reference: String,
        /// The id it named.
        target: String,
    },
    /// A reference names an operation that has not run by then — a later one,
    /// or itself.
    ForwardReference {
        /// The operation the reference was written in.
        op_id: String,
        /// The reference as written.
        reference: String,
        /// The id it named.
        target: String,
    },
    /// The expansion produced no steps at all, so the plan would do nothing.
    NothingToDo,
}

impl CompileError {
    /// Stable code for matching.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Expansion { .. } => "plan_expansion_failed",
            Self::BadReference { .. } => "plan_bad_reference",
            Self::UnknownReference { .. } => "plan_unknown_reference",
            Self::ForwardReference { .. } => "plan_forward_reference",
            Self::NothingToDo => "plan_nothing_to_do",
        }
    }

    /// The operation the failure is about, when it is about one.
    #[must_use]
    pub fn op_id(&self) -> Option<&str> {
        match self {
            Self::Expansion { op_id, .. }
            | Self::BadReference { op_id, .. }
            | Self::UnknownReference { op_id, .. }
            | Self::ForwardReference { op_id, .. } => Some(op_id),
            Self::NothingToDo => None,
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expansion { op_id, op, error } => {
                write!(
                    f,
                    "operation '{op_id}' ({op}) could not be expanded: {error}"
                )
            }
            Self::BadReference { op_id, error } => {
                write!(f, "operation '{op_id}': {error}")
            }
            Self::UnknownReference {
                op_id,
                reference,
                target,
            } => write!(
                f,
                "operation '{op_id}' refers to {reference}, but no operation '{target}' exists in \
                 this plan"
            ),
            Self::ForwardReference {
                op_id,
                reference,
                target,
            } => write!(
                f,
                "operation '{op_id}' refers to {reference}, but '{target}' does not run before it"
            ),
            Self::NothingToDo => write!(f, "the plan expanded to no tool calls"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Compile a plan into a runnable [`Program`].
///
/// # Errors
///
/// Returns the first [`CompileError`]. Nothing is executed, and nothing needs
/// to be undone: this runs before the plan touches anything.
pub fn compile(plan: &Plan, library: &dyn OpLibrary) -> Result<Program, CompileError> {
    // Expansion first, in full. Reference checking needs the complete id
    // universe to tell "that operation does not exist" from "that operation
    // runs later" — two mistakes with opposite fixes, rename versus reorder.
    let mut steps: Vec<Step> = Vec::new();
    let mut expansion: Vec<Expansion> = Vec::new();

    for op in &plan.ops {
        let specs = library
            .expand(op)
            .map_err(|error| CompileError::Expansion {
                op_id: op.id.clone(),
                op: op.op.clone(),
                error,
            })?;

        let multi = specs.len() > 1;
        for (index, spec) in specs.into_iter().enumerate() {
            steps.push(Step {
                id: if multi {
                    format!("{}/{}", op.id, index)
                } else {
                    op.id.clone()
                },
                op_id: op.id.clone(),
                op: op.op.clone(),
                tool: spec.tool,
                args: spec.args,
            });
        }

        let produced = steps.iter().filter(|s| s.op_id == op.id).count();
        expansion.push(Expansion {
            op_id: op.id.clone(),
            op: op.op.clone(),
            steps: produced,
        });
    }

    if steps.is_empty() {
        return Err(CompileError::NothingToDo);
    }

    let universe: Vec<&str> = plan
        .ops
        .iter()
        .map(|op| op.id.as_str())
        .chain(steps.iter().map(|s| s.id.as_str()))
        .collect();

    // Ids bound by the time each operation runs. The boundary is the operation,
    // not the step: a macro's steps are generated by a library that does not
    // know its own assigned prefix, so it cannot refer to them, and drawing the
    // line at the operation keeps the rule one sentence long.
    let mut available: Vec<&str> = Vec::with_capacity(universe.len());
    for op in &plan.ops {
        for step in steps.iter().filter(|s| s.op_id == op.id) {
            let found = refs::scan(&step.args).map_err(|error| CompileError::BadReference {
                op_id: op.id.clone(),
                error,
            })?;
            for reference in found {
                if available.contains(&reference.op.as_str()) {
                    continue;
                }
                return Err(if universe.contains(&reference.op.as_str()) {
                    CompileError::ForwardReference {
                        op_id: op.id.clone(),
                        reference: reference.to_string(),
                        target: reference.op.clone(),
                    }
                } else {
                    CompileError::UnknownReference {
                        op_id: op.id.clone(),
                        reference: reference.to_string(),
                        target: reference.op.clone(),
                    }
                });
            }
        }
        available.push(op.id.as_str());
        for step in steps.iter().filter(|s| s.op_id == op.id) {
            available.push(step.id.as_str());
        }
    }

    Ok(Program {
        plan_id: plan.plan_id.clone(),
        documents: plan.documents.clone(),
        base_revisions: plan.base_revisions.clone(),
        validators: plan.validators.clone(),
        constraints: plan.constraints.clone(),
        rollback_policy: plan.rollback_policy,
        steps,
        expansion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A library with exactly two behaviours: `call` passes through, and
    /// `twice` expands to two steps. Enough to exercise identity and ordering
    /// without pretending to know a domain.
    struct Fixture;

    impl OpLibrary for Fixture {
        fn expand(&self, op: &Op) -> Result<Vec<StepSpec>, ExpandError> {
            match op.op.as_str() {
                "call" => {
                    let tool = op.with["tool"]
                        .as_str()
                        .ok_or_else(|| ExpandError::invalid("tool", "required"))?;
                    Ok(vec![StepSpec {
                        tool: tool.to_string(),
                        args: op.with.get("args").cloned().unwrap_or(json!({})),
                    }])
                }
                "twice" => Ok(vec![
                    StepSpec {
                        tool: "first".to_string(),
                        args: op.with.clone(),
                    },
                    StepSpec {
                        tool: "second".to_string(),
                        args: op.with.clone(),
                    },
                ]),
                "nothing" => Ok(Vec::new()),
                other => Err(ExpandError::unknown_op(other)),
            }
        }
    }

    fn plan(value: Value) -> Plan {
        Plan::from_json(&value).unwrap()
    }

    #[test]
    fn one_operation_becomes_its_tool_call() {
        let program = compile(
            &plan(json!({"ops": [{"id": "a", "op": "call", "with": {"tool": "run_erc"}}]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps.len(), 1);
        assert_eq!(program.steps[0].tool, "run_erc");
        assert_eq!(program.steps[0].id, "a");
    }

    #[test]
    fn a_macro_becomes_several_addressable_steps() {
        let program = compile(
            &plan(json!({"ops": [{"id": "m", "op": "twice", "with": {"x": 1}}]})),
            &Fixture,
        )
        .unwrap();
        let ids: Vec<&str> = program.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["m/0", "m/1"]);
        assert_eq!(program.expansion[0].steps, 2);
    }

    #[test]
    fn a_backward_reference_compiles() {
        let program = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "b", "op": "call", "with": {"tool": "use", "args": {"r": "${a.reference}"}}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert!(program.has_references());
    }

    #[test]
    fn one_step_of_a_macro_can_be_referenced_by_id() {
        // `/` is legal in a reference target and illegal in an operation id, so
        // `m/0` can only ever mean a step.
        let program = compile(
            &plan(json!({"ops": [
                {"id": "m", "op": "twice", "with": {}},
                {"id": "b", "op": "call", "with": {"tool": "use", "args": {"r": "${m/0.uuid}"}}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps.len(), 3);
    }

    #[test]
    fn a_macro_cannot_refer_to_its_own_steps() {
        // The boundary is the operation: a library does not know the prefix it
        // will be given, so a reference to it can only be a mistake.
        let err = compile(
            &plan(json!({"ops": [
                {"id": "m", "op": "twice", "with": {"r": "${m/0.uuid}"}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_forward_reference");
    }

    #[test]
    fn a_forward_reference_is_refused_before_anything_runs() {
        // The whole point: a plan that cannot finish must not start.
        let err = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "use", "args": {"r": "${b.x}"}}},
                {"id": "b", "op": "call", "with": {"tool": "make"}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_forward_reference");
        assert_eq!(err.op_id(), Some("a"));
    }

    #[test]
    fn an_operation_cannot_refer_to_itself() {
        let err = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "use", "args": {"r": "${a.x}"}}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_forward_reference");
    }

    #[test]
    fn a_reference_to_nothing_is_named_differently_from_one_to_a_later_step() {
        // The two demand different fixes: rename versus reorder.
        let err = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "use", "args": {"r": "${ghost.x}"}}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_unknown_reference");
    }

    #[test]
    fn an_unknown_operation_names_itself() {
        let err = compile(&plan(json!({"ops": [{"op": "levitate"}]})), &Fixture).unwrap_err();
        assert_eq!(err.code(), "plan_expansion_failed");
        assert!(err.to_string().contains("levitate"));
    }

    #[test]
    fn a_plan_that_expands_to_nothing_is_refused() {
        let err = compile(&plan(json!({"ops": [{"op": "nothing"}]})), &Fixture).unwrap_err();
        assert_eq!(err.code(), "plan_nothing_to_do");
    }

    #[test]
    fn a_reference_free_program_says_so() {
        let program = compile(
            &plan(json!({"ops": [{"op": "call", "with": {"tool": "run_erc"}}]})),
            &Fixture,
        )
        .unwrap();
        assert!(!program.has_references());
        assert_eq!(program.to_calls(), json!([{"tool": "run_erc", "args": {}}]));
    }

    #[test]
    fn the_summary_says_what_expanded_into_what() {
        let program = compile(
            &plan(json!({"plan_id": "p1", "validators": ["erc"], "ops": [
                {"id": "a", "op": "call", "with": {"tool": "x"}},
                {"id": "m", "op": "twice", "with": {}}
            ]})),
            &Fixture,
        )
        .unwrap();
        let summary = program.summary();
        assert_eq!(summary["ops"], json!(2));
        assert_eq!(summary["steps"], json!(3));
        assert_eq!(summary["expansion"][1], json!("twice=2 step(s)"));
        assert_eq!(summary["validators"], json!(["erc"]));
    }
}
