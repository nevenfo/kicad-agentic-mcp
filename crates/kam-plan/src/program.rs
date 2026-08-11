//! The compiled plan — the exact tool calls that will run, in order.
//!
//! A [`Program`] is the artefact that makes a plan reviewable. The plan says
//! `decouple U1`; the program says which four capacitors get placed, where, and
//! what gets wired to what. That is the difference between trusting the
//! expansion and being able to check it, and it is why previewing a plan is a
//! first-class operation rather than a debug flag: the same compile that would
//! run produces the listing, so what is reviewed is what executes.

use crate::ir::RollbackPolicy;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// One tool call produced by expanding an operation.
///
/// The op library returns these; it does not name them. Identity is assigned by
/// the compiler, so an expansion cannot accidentally collide with an id a
/// reference depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct StepSpec {
    /// Tool to call.
    pub tool: String,
    /// Arguments, possibly still containing `${...}` references.
    pub args: Value,
}

/// A compiled step: one tool call, addressable, traceable to its operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Step identity — the operation id for a single-step operation, or
    /// `op_id/n` for the n-th step of a macro.
    pub id: String,
    /// The operation that produced it, for reporting.
    pub op_id: String,
    /// The operation's name, for reporting.
    pub op: String,
    /// Tool to call.
    pub tool: String,
    /// Arguments, possibly containing `${...}` references.
    pub args: Value,
}

impl Step {
    /// The step as a `{tool, args}` call, the shape a batch executor expects.
    #[must_use]
    pub fn to_call(&self) -> Value {
        let mut map = Map::new();
        map.insert("tool".to_string(), Value::String(self.tool.clone()));
        map.insert("args".to_string(), self.args.clone());
        Value::Object(map)
    }
}

/// A plan after expansion: nothing left to decide, only to do.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The plan's identity, carried through for evidence.
    pub plan_id: String,
    /// Documents the plan declared.
    pub documents: Vec<String>,
    /// Preconditions the host is expected to check.
    pub base_revisions: BTreeMap<String, String>,
    /// Validators to run afterwards.
    pub validators: Vec<String>,
    /// Constraints, carried for the record.
    pub constraints: Vec<String>,
    /// What happens to completed work when a step fails.
    pub rollback_policy: RollbackPolicy,
    /// The steps, in execution order.
    pub steps: Vec<Step>,
    /// How many steps each operation expanded to, in plan order.
    pub expansion: Vec<Expansion>,
}

/// How one declared operation became tool calls.
#[derive(Debug, Clone, PartialEq)]
pub struct Expansion {
    /// The operation's id.
    pub op_id: String,
    /// The operation's name.
    pub op: String,
    /// How many steps it produced.
    pub steps: usize,
}

impl Program {
    /// A compact description: what will run, without the arguments.
    ///
    /// This is the summary a caller reads before approving. The full listing is
    /// [`Program::to_calls`], which is bigger and is what a reviewer asks for
    /// when the summary is not enough.
    #[must_use]
    pub fn summary(&self) -> Value {
        let mut map = Map::new();
        if !self.plan_id.is_empty() {
            map.insert("plan_id".to_string(), Value::String(self.plan_id.clone()));
        }
        map.insert("ops".to_string(), Value::from(self.expansion.len()));
        map.insert("steps".to_string(), Value::from(self.steps.len()));
        map.insert(
            "expansion".to_string(),
            Value::Array(
                self.expansion
                    .iter()
                    .map(|e| Value::String(format!("{}={} step(s)", e.op, e.steps)))
                    .collect(),
            ),
        );
        map.insert(
            "rollback_policy".to_string(),
            Value::String(self.rollback_policy.as_str().to_string()),
        );
        if !self.validators.is_empty() {
            map.insert(
                "validators".to_string(),
                Value::from(self.validators.clone()),
            );
        }
        if !self.constraints.is_empty() {
            map.insert(
                "constraints".to_string(),
                Value::from(self.constraints.clone()),
            );
        }
        Value::Object(map)
    }

    /// The steps as `{tool, args}` calls — the whole listing.
    #[must_use]
    pub fn to_calls(&self) -> Value {
        Value::Array(self.steps.iter().map(Step::to_call).collect())
    }

    /// Whether any step still carries an unresolved reference.
    ///
    /// A program with no references is a plain batch, and a host may hand it
    /// straight to an existing batch executor rather than driving it step by
    /// step. That is not an optimisation this crate performs; it is a fact it
    /// reports so the host can.
    #[must_use]
    pub fn has_references(&self) -> bool {
        self.steps
            .iter()
            .any(|step| crate::refs::scan(&step.args).is_ok_and(|found| !found.is_empty()))
    }
}
