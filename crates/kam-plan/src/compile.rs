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
use serde_json::{Map, Value};
use std::collections::HashMap;

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
    /// A reference used a bare number (`${1.field}`) that names two earlier
    /// operations at once: the one at that zero-based position, and the one
    /// whose id is literally that digit string. Resolved by elimination when
    /// only one of the two exists and runs before this operation — this error
    /// is only for the case where both do, or where the plan gave an
    /// operation a purely numeric id on purpose. Write `${ops[N].field}` for
    /// position N (zero-based) or name the id unambiguously.
    AmbiguousNumericReference {
        /// The operation the reference was written in.
        op_id: String,
        /// The reference as written.
        reference: String,
        /// Every operation id in the plan, in declared order.
        available: Vec<String>,
    },
    /// A reference named an operation *type* (`${create.field}`) that more
    /// than one operation in the plan has, so which one is meant cannot be
    /// inferred.
    AmbiguousOpTypeReference {
        /// The operation the reference was written in.
        op_id: String,
        /// The reference as written.
        reference: String,
        /// The operation type it named.
        op_type: String,
        /// The ids of every operation with that type.
        candidates: Vec<String>,
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
            Self::AmbiguousNumericReference { .. } => "plan_ambiguous_numeric_reference",
            Self::AmbiguousOpTypeReference { .. } => "plan_ambiguous_op_type_reference",
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
            | Self::ForwardReference { op_id, .. }
            | Self::AmbiguousNumericReference { op_id, .. }
            | Self::AmbiguousOpTypeReference { op_id, .. } => Some(op_id),
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
            Self::AmbiguousNumericReference {
                op_id,
                reference,
                available,
            } => write!(
                f,
                "operation '{op_id}' refers to {reference} with a bare number, which is \
                 ambiguous between a position and an id; write ${{ops[N].field}} for position N \
                 (zero-based), or name an id — ids in this plan: {}",
                available.join(", ")
            ),
            Self::AmbiguousOpTypeReference {
                op_id,
                reference,
                op_type,
                candidates,
            } => write!(
                f,
                "operation '{op_id}' refers to {reference} by type '{op_type}', but {} \
                 operations have that type ({}); name one of them by id instead",
                candidates.len(),
                candidates.join(", ")
            ),
            Self::NothingToDo => write!(f, "the plan expanded to no tool calls"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Fill `with` from `defaults`, one top-level key at a time, without
/// touching a key `with` already has — an explicit `null` counts as having
/// it. `with` is always an object by the time this runs ([`Plan::from_json`]
/// refuses anything else), but a non-object is returned unchanged rather than
/// assumed away, since a defensive check here is cheaper than a panic
/// somewhere that trusted it.
fn apply_defaults(with: &Value, defaults: &Map<String, Value>) -> Value {
    if defaults.is_empty() {
        return with.clone();
    }
    let Value::Object(existing) = with else {
        return with.clone();
    };
    let mut merged = existing.clone();
    for (key, value) in defaults {
        if !merged.contains_key(key) {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

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
        // `defaults` is filled in here, per operation, so the library never
        // sees the difference between a key the plan wrote out and one it
        // left to the default — both arrive the same way, in `with`.
        let effective = Op {
            id: op.id.clone(),
            op: op.op.clone(),
            with: apply_defaults(&op.with, &plan.defaults),
        };
        let specs = library
            .expand(&effective)
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

    // Owned, not borrowed: the rewrite pass just below needs `steps` mutably,
    // and a `&str` universe borrowed from it would conflict with that.
    let universe: Vec<String> = plan
        .ops
        .iter()
        .map(|op| op.id.clone())
        .chain(steps.iter().map(|s| s.id.clone()))
        .collect();

    // Every operation id, by position — what `${ops[N].field}` names.
    let by_position: Vec<&str> = plan.ops.iter().map(|op| op.id.as_str()).collect();

    // Every id an operation *type* resolves to, so `${create.field}` can name
    // the one operation the plan actually has of that type. Built once over
    // the whole plan: uniqueness is a property of the plan, not of how far
    // compilation has got.
    let mut by_type: HashMap<&str, Vec<&str>> = HashMap::new();
    for op in &plan.ops {
        by_type
            .entry(op.op.as_str())
            .or_default()
            .push(op.id.as_str());
    }

    // Canonicalise every tolerant reference form — `${ops[N].field}`,
    // `${op_type.field}` when it names exactly one operation — into the
    // plain `${op_id.field}` the rest of compilation, and everything at run
    // time, already understands. Nothing past this point needs to know the
    // tolerant forms exist.
    for (op_index, op) in plan.ops.iter().enumerate() {
        for step in steps.iter_mut().filter(|s| s.op_id == op.id) {
            let op_id = op.id.clone();
            let mut resolve_target = |reference: &refs::Reference| -> Result<String, CompileError> {
                let target = reference.op.as_str();

                // A bare number has two possible meanings: the operation at
                // that zero-based position, or an operation whose id is
                // literally that digit string. Resolved by elimination: only
                // a candidate that both exists *and* runs strictly before
                // this operation counts, so a reference forward or to itself
                // is never silently accepted here — it falls through to the
                // ordinary unknown/forward-reference checks below. If both
                // candidates survive, which one was meant cannot be known,
                // and that stays refused.
                if !target.is_empty() && target.chars().all(|c| c.is_ascii_digit()) {
                    let position_candidate = target
                        .parse::<usize>()
                        .ok()
                        .filter(|&n| n < op_index)
                        .and_then(|n| by_position.get(n))
                        .copied();
                    let id_candidate = by_position[..op_index].contains(&target).then_some(target);

                    match (position_candidate, id_candidate) {
                        (Some(id), None) | (None, Some(id)) => return Ok(id.to_string()),
                        (Some(_), Some(_)) => {
                            return Err(CompileError::AmbiguousNumericReference {
                                op_id: op_id.clone(),
                                reference: reference.to_string(),
                                available: by_position.iter().map(|id| id.to_string()).collect(),
                            });
                        }
                        (None, None) => {} // neither before this op — fall through
                    }
                }

                if let Some(position) = refs::position_target(target) {
                    return Ok(by_position
                        .get(position)
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| target.to_string()));
                }

                if universe.iter().any(|id| id == target) {
                    return Ok(target.to_string());
                }

                match by_type.get(target) {
                    Some(candidates) if candidates.len() == 1 => Ok(candidates[0].to_string()),
                    Some(candidates) if candidates.len() > 1 => {
                        Err(CompileError::AmbiguousOpTypeReference {
                            op_id: op_id.clone(),
                            reference: reference.to_string(),
                            op_type: target.to_string(),
                            candidates: candidates.iter().map(|id| id.to_string()).collect(),
                        })
                    }
                    _ => Ok(target.to_string()),
                }
            };

            step.args =
                refs::rewrite(&step.args, &mut resolve_target).map_err(|error| match error {
                    refs::RewriteError::Ref(error) => CompileError::BadReference {
                        op_id: op.id.clone(),
                        error,
                    },
                    refs::RewriteError::Target(error) => error,
                })?;
        }
    }

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
                return Err(if universe.contains(&reference.op) {
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
    fn defaults_fill_a_key_the_operation_did_not_give() {
        let program = compile(
            &plan(json!({
                "defaults": {"tool": "run_erc"},
                "ops": [{"op": "call", "with": {}}]
            })),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[0].tool, "run_erc");
    }

    #[test]
    fn defaults_never_overwrite_a_key_the_operation_gave_even_an_explicit_null() {
        let program = compile(
            &plan(json!({
                "defaults": {"args": {"from": "default"}},
                "ops": [{"op": "call", "with": {"tool": "x", "args": null}}]
            })),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[0].args, json!(null));
    }

    #[test]
    fn a_positional_reference_resolves_to_the_operation_at_that_index() {
        let program = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "b", "op": "call", "with": {"tool": "use", "args": {"r": "${ops[0].reference}"}}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[1].args, json!({"r": "${a.reference}"}));
    }

    #[test]
    fn a_unique_op_type_reference_resolves_to_its_id() {
        let program = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "b", "op": "twice", "with": {"r": "${call.reference}"}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[1].args, json!({"r": "${a.reference}"}));
        assert_eq!(program.steps[2].args, json!({"r": "${a.reference}"}));
    }

    #[test]
    fn an_ambiguous_op_type_reference_names_both_candidates() {
        let err = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "c", "op": "call", "with": {"tool": "make2"}},
                {"id": "b", "op": "twice", "with": {"r": "${call.reference}"}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_ambiguous_op_type_reference");
        assert!(err.to_string().contains("(a, c)"), "{err}");
    }

    #[test]
    fn a_bare_zero_resolves_to_the_first_operation_by_position() {
        // No auto-assigned id is ever "0" (ids start at "op1"), so the id
        // reading never survives and position wins by elimination.
        let program = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "b", "op": "call", "with": {"tool": "use", "args": {"r": "${0.reference}"}}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[1].args, json!({"r": "${a.reference}"}));
    }

    #[test]
    fn a_bare_number_resolves_when_only_one_reading_is_a_valid_earlier_operation() {
        // "${1.field}" here can only mean position 1 ("b"): no operation is
        // literally named "1".
        let program = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "b", "op": "call", "with": {"tool": "make"}},
                {"id": "c", "op": "call", "with": {"tool": "use", "args": {"r": "${1.reference}"}}}
            ]})),
            &Fixture,
        )
        .unwrap();
        assert_eq!(program.steps[2].args, json!({"r": "${b.reference}"}));
    }

    #[test]
    fn a_bare_number_is_refused_when_both_readings_are_valid_earlier_operations() {
        // Position 1 is "second"; an earlier operation is also literally
        // named "1" — two different operations, so "${1.field}" cannot be
        // resolved by elimination.
        let err = compile(
            &plan(json!({"ops": [
                {"id": "1", "op": "call", "with": {"tool": "make"}},
                {"id": "second", "op": "call", "with": {"tool": "make"}},
                {"id": "c", "op": "call", "with": {"tool": "use", "args": {"r": "${1.reference}"}}}
            ]})),
            &Fixture,
        )
        .unwrap_err();
        assert_eq!(err.code(), "plan_ambiguous_numeric_reference");
        assert!(err.to_string().contains("1, second, c"), "{err}");
    }

    #[test]
    fn a_bare_number_naming_no_earlier_operation_falls_through_unresolved() {
        // Position 1 is "c" itself (not strictly earlier), and no operation
        // is literally named "1" — neither reading survives, so this is not
        // the ambiguous-numeric case; it falls through to the ordinary
        // unknown-reference check, unchanged.
        let err = compile(
            &plan(json!({"ops": [
                {"id": "a", "op": "call", "with": {"tool": "make"}},
                {"id": "c", "op": "call", "with": {"tool": "use", "args": {"r": "${1.reference}"}}}
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
