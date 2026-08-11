//! Running a compiled program, one resolved step at a time.
//!
//! Execution is a state machine rather than a loop that owns the tool caller,
//! for one practical reason: the host is async and this crate is not. The host
//! asks for the next step, runs it however it runs things, and hands back what
//! it got:
//!
//! ```text
//! while let Next::Step(step) = run.next_step() {
//!     let output = call(step.tool, step.args).await;
//!     run.record(output.ok, output.body);
//! }
//! ```
//!
//! That keeps this crate free of an async runtime, a trait object with a
//! lifetime, and any opinion about how a tool is invoked — and it makes the
//! whole thing testable without spinning up a reactor.
//!
//! What the state machine owns is the part that must not be re-derived per host:
//! resolving each step's references against outputs already recorded, deciding
//! whether a failure stops the run, and reporting what happened in a form that
//! can be filed as evidence.

use crate::ir::RollbackPolicy;
use crate::program::Program;
use crate::refs::{self, Outputs};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A step with its references substituted — ready to call.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStep {
    /// Position in the program.
    pub index: usize,
    /// Step identity.
    pub id: String,
    /// Originating operation name.
    pub op: String,
    /// Tool to call.
    pub tool: String,
    /// Arguments, fully resolved.
    pub args: Value,
}

/// What happened to one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// Position in the program.
    pub index: usize,
    /// Step identity.
    pub id: String,
    /// Originating operation name.
    pub op: String,
    /// Tool that was called, or would have been.
    pub tool: String,
    /// Whether it succeeded.
    pub ok: bool,
    /// Whether it never ran because its references could not be resolved.
    pub unresolved: bool,
    /// What the tool returned, or the reason it was not called.
    pub output: Value,
}

/// The next thing the host should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Call this, then [`Execution::record`] the outcome.
    Step(ResolvedStep),
    /// A step could not be resolved and was not called. Already recorded; call
    /// [`Execution::next_step`] again to find out whether the run continues.
    Unresolved(StepResult),
    /// Nothing left, either because the program finished or because a failure
    /// stopped it.
    Done,
}

/// A program in progress.
pub struct Execution {
    program: Program,
    outputs: BTreeMap<String, Value>,
    results: Vec<StepResult>,
    cursor: usize,
    pending: Option<ResolvedStep>,
    halted: bool,
}

impl Execution {
    /// Start a run. Nothing happens until [`Execution::next_step`] is called.
    #[must_use]
    pub fn new(program: Program) -> Self {
        Self {
            program,
            outputs: BTreeMap::new(),
            results: Vec::new(),
            cursor: 0,
            pending: None,
            halted: false,
        }
    }

    /// The program being run.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// The next step to call.
    ///
    /// Calling this twice without a [`Execution::record`] in between returns
    /// the same step: the cursor advances on the record, not on the ask, so a
    /// host that loses track cannot skip a step by asking again.
    pub fn next_step(&mut self) -> Next {
        if let Some(step) = &self.pending {
            return Next::Step(step.clone());
        }
        if self.halted || self.cursor >= self.program.steps.len() {
            return Next::Done;
        }

        let step = &self.program.steps[self.cursor];
        match refs::resolve(&step.args, &self.outputs as &dyn Outputs) {
            Ok(args) => {
                let resolved = ResolvedStep {
                    index: self.cursor,
                    id: step.id.clone(),
                    op: step.op.clone(),
                    tool: step.tool.clone(),
                    args,
                };
                self.pending = Some(resolved.clone());
                Next::Step(resolved)
            }
            Err(error) => {
                // The compiler proved the operation exists and runs earlier;
                // what it could not prove is that the output would contain the
                // field. So this is a run-time failure of a specific step, with
                // the reference named, rather than a mystery.
                let result = StepResult {
                    index: self.cursor,
                    id: step.id.clone(),
                    op: step.op.clone(),
                    tool: step.tool.clone(),
                    ok: false,
                    unresolved: true,
                    output: Value::Object({
                        let mut map = Map::new();
                        map.insert(
                            "error_kind".to_string(),
                            Value::String(error.code().to_string()),
                        );
                        map.insert("error".to_string(), Value::String(error.to_string()));
                        map
                    }),
                };
                self.results.push(result.clone());
                self.cursor += 1;
                if self.program.rollback_policy.halts() {
                    self.halted = true;
                }
                Next::Unresolved(result)
            }
        }
    }

    /// Record the outcome of the step [`Execution::next_step`] handed out.
    ///
    /// A record with no pending step is ignored rather than fatal: a host that
    /// double-records has a bug, but corrupting the run's history would make
    /// that bug harder to see than dropping the duplicate.
    pub fn record(&mut self, ok: bool, output: Value) {
        let Some(step) = self.pending.take() else {
            return;
        };

        // Bind under both addresses. The step id is exact; the operation id
        // resolves to its last step, which is the one a reader means by "what
        // that operation produced".
        self.outputs.insert(step.id.clone(), output.clone());
        let op_id = self.program.steps[step.index].op_id.clone();
        self.outputs.insert(op_id, output.clone());

        self.results.push(StepResult {
            index: step.index,
            id: step.id,
            op: step.op,
            tool: step.tool,
            ok,
            unresolved: false,
            output,
        });
        self.cursor += 1;
        if !ok && self.program.rollback_policy.halts() {
            self.halted = true;
        }
    }

    /// Whether the run is over.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.pending.is_none() && (self.halted || self.cursor >= self.program.steps.len())
    }

    /// What happened.
    #[must_use]
    pub fn report(&self) -> ExecutionReport {
        let ok = self.results.iter().filter(|r| r.ok).count();
        let failed_at = self.results.iter().find(|r| !r.ok).map(|r| r.index);
        ExecutionReport {
            plan_id: self.program.plan_id.clone(),
            steps: self.program.steps.len(),
            ran: self.results.len(),
            ok,
            failed_at,
            not_run: self.program.steps.len().saturating_sub(self.results.len()),
            rollback: failed_at.is_some() && self.program.rollback_policy.rolls_back(),
            rollback_policy: self.program.rollback_policy,
            results: self.results.clone(),
        }
    }
}

/// The outcome of a run.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    /// The plan's identity.
    pub plan_id: String,
    /// How many steps the program had.
    pub steps: usize,
    /// How many were attempted.
    pub ran: usize,
    /// How many succeeded.
    pub ok: usize,
    /// Index of the first failure.
    pub failed_at: Option<usize>,
    /// How many steps were never attempted.
    pub not_run: usize,
    /// Whether the policy calls for undoing the completed work.
    pub rollback: bool,
    /// The policy in force.
    pub rollback_policy: RollbackPolicy,
    /// Per-step outcomes.
    pub results: Vec<StepResult>,
}

impl ExecutionReport {
    /// A compact JSON body, the shape a caller reads first.
    ///
    /// Per-step output is deliberately excluded: it is what makes a reply grow
    /// with the size of the plan, and the whole point of a plan is that the
    /// caller does not have to read every step to know what happened. A host
    /// that wants the detail has [`ExecutionReport::results`].
    #[must_use]
    pub fn to_summary(&self) -> Value {
        let mut map = Map::new();
        if !self.plan_id.is_empty() {
            map.insert("plan_id".to_string(), Value::String(self.plan_id.clone()));
        }
        map.insert("steps".to_string(), Value::from(self.steps));
        map.insert("ok".to_string(), Value::from(self.ok));
        if let Some(index) = self.failed_at {
            map.insert("failed_at".to_string(), Value::from(index));
            if let Some(result) = self.results.iter().find(|r| !r.ok) {
                map.insert("failed_step".to_string(), Value::String(result.id.clone()));
                map.insert("failed_op".to_string(), Value::String(result.op.clone()));
            }
            if self.not_run > 0 {
                map.insert("not_run".to_string(), Value::from(self.not_run));
            }
            map.insert("rollback".to_string(), Value::Bool(self.rollback));
        }
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile, ExpandError, OpLibrary};
    use crate::ir::{Op, Plan};
    use crate::program::StepSpec;
    use serde_json::json;

    struct Fixture;

    impl OpLibrary for Fixture {
        fn expand(&self, op: &Op) -> Result<Vec<StepSpec>, ExpandError> {
            match op.op.as_str() {
                "call" => Ok(vec![StepSpec {
                    tool: op.with["tool"].as_str().unwrap_or("noop").to_string(),
                    args: op.with.get("args").cloned().unwrap_or(json!({})),
                }]),
                "twice" => Ok(vec![
                    StepSpec {
                        tool: "first".to_string(),
                        args: json!({}),
                    },
                    StepSpec {
                        tool: "second".to_string(),
                        args: json!({}),
                    },
                ]),
                other => Err(ExpandError::unknown_op(other)),
            }
        }
    }

    fn run_of(value: Value) -> Execution {
        let plan = Plan::from_json(&value).unwrap();
        Execution::new(compile(&plan, &Fixture).unwrap())
    }

    #[test]
    fn steps_are_handed_out_in_order_and_the_run_ends() {
        let mut run = run_of(json!({"ops": [
            {"op": "call", "with": {"tool": "a"}},
            {"op": "call", "with": {"tool": "b"}}
        ]}));

        let Next::Step(first) = run.next_step() else {
            panic!("expected a step")
        };
        assert_eq!(first.tool, "a");
        run.record(true, json!({}));

        let Next::Step(second) = run.next_step() else {
            panic!("expected a step")
        };
        assert_eq!(second.tool, "b");
        run.record(true, json!({}));

        assert_eq!(run.next_step(), Next::Done);
        assert!(run.finished());
        assert_eq!(run.report().ok, 2);
    }

    #[test]
    fn asking_twice_without_recording_returns_the_same_step() {
        // A host that retries the ask must not skip work.
        let mut run = run_of(json!({"ops": [
            {"op": "call", "with": {"tool": "a"}},
            {"op": "call", "with": {"tool": "b"}}
        ]}));
        assert_eq!(run.next_step(), run.next_step());
        run.record(true, json!({}));
        let Next::Step(next) = run.next_step() else {
            panic!()
        };
        assert_eq!(next.tool, "b");
    }

    #[test]
    fn an_output_reaches_the_next_step_without_a_round_trip() {
        let mut run = run_of(json!({"ops": [
            {"id": "make", "op": "call", "with": {"tool": "add"}},
            {"op": "call", "with": {"tool": "use", "args": {"ref": "${make.reference}", "at": "${make.x}"}}}
        ]}));

        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(true, json!({"reference": "R1", "x": 100.33}));

        let Next::Step(second) = run.next_step() else {
            panic!()
        };
        assert_eq!(second.args, json!({"ref": "R1", "at": 100.33}));
    }

    #[test]
    fn a_macro_binds_its_operation_id_to_its_last_step() {
        let mut run = run_of(json!({"ops": [
            {"id": "m", "op": "twice"},
            {"op": "call", "with": {"tool": "use", "args": {"v": "${m.tag}", "w": "${m/0.tag}"}}}
        ]}));

        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(true, json!({"tag": "one"}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(true, json!({"tag": "two"}));

        let Next::Step(third) = run.next_step() else {
            panic!()
        };
        assert_eq!(third.args, json!({"v": "two", "w": "one"}));
    }

    #[test]
    fn a_failure_stops_the_run_under_the_default_policy() {
        let mut run = run_of(json!({"ops": [
            {"op": "call", "with": {"tool": "a"}},
            {"op": "call", "with": {"tool": "b"}}
        ]}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(false, json!({"error": "no"}));

        assert_eq!(run.next_step(), Next::Done);
        let report = run.report();
        assert_eq!(report.failed_at, Some(0));
        assert_eq!(report.not_run, 1);
        assert!(report.rollback);
    }

    #[test]
    fn continue_keeps_going_and_does_not_roll_back() {
        let mut run = run_of(json!({"rollback_policy": "continue", "ops": [
            {"op": "call", "with": {"tool": "a"}},
            {"op": "call", "with": {"tool": "b"}}
        ]}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(false, json!({}));
        let Next::Step(second) = run.next_step() else {
            panic!("continue should hand out the next step")
        };
        assert_eq!(second.tool, "b");
        run.record(true, json!({}));

        let report = run.report();
        assert_eq!(report.ran, 2);
        assert!(!report.rollback);
    }

    #[test]
    fn stop_on_error_halts_without_rolling_back() {
        let mut run = run_of(json!({"rollback_policy": "stop_on_error", "ops": [
            {"op": "call", "with": {"tool": "a"}},
            {"op": "call", "with": {"tool": "b"}}
        ]}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(false, json!({}));
        assert_eq!(run.next_step(), Next::Done);
        assert!(!run.report().rollback);
    }

    #[test]
    fn a_field_that_never_arrived_fails_its_own_step_by_name() {
        // The compiler proved 'make' runs first; it could not prove the tool
        // would return a 'reference'. So the failure is specific.
        let mut run = run_of(json!({"ops": [
            {"id": "make", "op": "call", "with": {"tool": "add"}},
            {"op": "call", "with": {"tool": "use", "args": {"r": "${make.reference}"}}}
        ]}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(true, json!({"nothing": true}));

        let Next::Unresolved(failure) = run.next_step() else {
            panic!("expected an unresolved step")
        };
        assert!(failure.unresolved);
        assert_eq!(failure.output["error_kind"], json!("ref_no_such_path"));
        assert!(failure.output["error"]
            .as_str()
            .unwrap()
            .contains("${make.reference}"));
        assert_eq!(run.next_step(), Next::Done);
        assert!(run.report().rollback);
    }

    #[test]
    fn the_summary_is_short_when_nothing_went_wrong() {
        let mut run =
            run_of(json!({"plan_id": "p9", "ops": [{"op": "call", "with": {"tool": "a"}}]}));
        let Next::Step(_) = run.next_step() else {
            panic!()
        };
        run.record(
            true,
            json!({"big": "payload that does not belong in the summary"}),
        );
        assert_eq!(
            run.report().to_summary(),
            json!({"plan_id": "p9", "steps": 1, "ok": 1})
        );
    }

    #[test]
    fn recording_without_a_pending_step_is_ignored() {
        let mut run = run_of(json!({"ops": [{"op": "call", "with": {"tool": "a"}}]}));
        run.record(true, json!({}));
        assert_eq!(run.report().ran, 0);
    }
}
