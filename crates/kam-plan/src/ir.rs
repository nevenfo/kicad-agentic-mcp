//! The plan document — what a caller writes, before anything is expanded.
//!
//! Everything here is declarative and inert. A `Plan` is not runnable: it names
//! operations, not tool calls, and the operations may be macros that do not
//! exist yet as far as this crate is concerned. [`crate::compile`] is what turns
//! it into something that can run.
//!
//! The parser is strict on purpose. A plan is usually written by a model, and
//! the cost of a bad plan is a retry — so every rejection names the field it is
//! about and carries a stable code, which is the difference between a retry that
//! fixes the problem and a retry that guesses.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Version of the plan IR this crate reads and writes.
///
/// A plan may omit it, in which case it is assumed current. A plan that names a
/// *different* version is refused rather than interpreted: silently reading a
/// future dialect with today's rules is how a change gets misapplied.
pub const IR_VERSION: u64 = 1;

/// What to do with the work already done when an operation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RollbackPolicy {
    /// Undo the whole plan. The default: a plan is one unit of work.
    #[default]
    Atomic,
    /// Stop at the failure but keep what succeeded.
    StopOnError,
    /// Keep going. For a plan whose operations are genuinely independent.
    Continue,
}

impl RollbackPolicy {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Atomic => "atomic",
            Self::StopOnError => "stop_on_error",
            Self::Continue => "continue",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "atomic" => Some(Self::Atomic),
            "stop_on_error" => Some(Self::StopOnError),
            "continue" => Some(Self::Continue),
            _ => None,
        }
    }

    /// Whether a failure means the work already done is thrown away.
    #[must_use]
    pub fn rolls_back(self) -> bool {
        self == Self::Atomic
    }

    /// Whether a failure stops the remaining operations.
    #[must_use]
    pub fn halts(self) -> bool {
        self != Self::Continue
    }
}

/// One declared operation.
///
/// `op` names something in the caller's [`OpLibrary`](crate::compile::OpLibrary);
/// `with` is its parameters, uninterpreted here. `id` is how a later operation
/// refers to this one's output, and it is always present after parsing — an
/// operation that did not supply one is given `op1`, `op2`, … by position, so a
/// reference always has something to point at.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    /// Identifier, unique within the plan.
    pub id: String,
    /// Operation name, resolved by the op library at compile time.
    pub op: String,
    /// Parameters. May contain `${...}` references to earlier operations.
    pub with: Value,
}

/// A declared change: operations, what they may assume, and how they unwind.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Caller-supplied identity, for logs and evidence. Empty if not given.
    pub plan_id: String,
    /// IR version the plan was written against.
    pub ir_version: u64,
    /// Documents the plan intends to touch, for callers that snapshot before
    /// running. Advisory: the executor is free to protect more.
    pub documents: Vec<String>,
    /// Preconditions on document state — `path -> revision`. Checked by the
    /// host, which is the only party that knows how to read a revision.
    pub base_revisions: BTreeMap<String, String>,
    /// Values filled into every operation's `with` for whatever top-level key
    /// that operation did not itself give — never overwriting a key the
    /// operation supplied, an explicit `null` included. Generic: this crate
    /// does not know or care what any key means, only that "absent" and
    /// "present" are different things. The common use is one path repeated
    /// across every operation, written once instead of in each `with`.
    pub defaults: Map<String, Value>,
    /// The operations, in order.
    pub ops: Vec<Op>,
    /// Free-text invariants the plan must not break. Carried, not enforced:
    /// enforcement is a validator's job, and pretending otherwise would be the
    /// kind of unearned "OK" this project exists to avoid.
    pub constraints: Vec<String>,
    /// Validators to run after the plan, by name.
    pub validators: Vec<String>,
    /// What happens to completed work when an operation fails.
    pub rollback_policy: RollbackPolicy,
}

/// Why a plan document was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The plan was not a JSON object.
    NotAnObject,
    /// A required field was missing.
    Missing {
        /// Field path, e.g. `ops[2].op`.
        field: String,
    },
    /// A field was present with the wrong type.
    WrongType {
        /// Field path.
        field: String,
        /// What was expected.
        expected: &'static str,
    },
    /// `ir_version` named a dialect this crate does not read.
    UnsupportedVersion {
        /// The version found.
        found: u64,
    },
    /// Two operations claimed the same id, so a reference to it is ambiguous.
    DuplicateOpId {
        /// The repeated id.
        id: String,
    },
    /// An operation id contained a character that would make a reference to it
    /// unparseable.
    BadOpId {
        /// The offending id.
        id: String,
    },
    /// `rollback_policy` was not one of the known values.
    UnknownPolicy {
        /// The value found.
        found: String,
    },
    /// The plan declared no operations.
    Empty,
}

impl PlanError {
    /// Stable code for matching, independent of the message's wording.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotAnObject => "plan_not_an_object",
            Self::Missing { .. } => "plan_missing_field",
            Self::WrongType { .. } => "plan_wrong_type",
            Self::UnsupportedVersion { .. } => "plan_unsupported_version",
            Self::DuplicateOpId { .. } => "plan_duplicate_op_id",
            Self::BadOpId { .. } => "plan_bad_op_id",
            Self::UnknownPolicy { .. } => "plan_unknown_policy",
            Self::Empty => "plan_empty",
        }
    }

    /// The field this is about, when it is about one.
    #[must_use]
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Missing { field } | Self::WrongType { field, .. } => Some(field),
            _ => None,
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "a plan must be a JSON object"),
            Self::Missing { field } => write!(f, "missing required field '{field}'"),
            Self::WrongType { field, expected } => {
                write!(f, "field '{field}' must be {expected}")
            }
            Self::UnsupportedVersion { found } => write!(
                f,
                "plan ir_version {found} is not supported (this build reads {IR_VERSION})"
            ),
            Self::DuplicateOpId { id } => {
                write!(f, "two operations share the id '{id}'; ids must be unique")
            }
            Self::BadOpId { id } => write!(
                f,
                "operation id '{id}' must be non-empty and use only letters, digits, '_' and '-'"
            ),
            Self::UnknownPolicy { found } => write!(
                f,
                "rollback_policy '{found}' is not one of atomic, stop_on_error, continue"
            ),
            Self::Empty => write!(f, "a plan needs at least one operation"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Whether an id can appear inside `${...}` without making it ambiguous.
#[must_use]
pub fn is_valid_op_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Plan {
    /// Parse a plan document.
    ///
    /// # Errors
    ///
    /// Returns a [`PlanError`] naming the offending field. Nothing is
    /// interpreted leniently: a plan is cheap to rewrite and expensive to
    /// misread.
    pub fn from_json(value: &Value) -> Result<Self, PlanError> {
        let Some(map) = value.as_object() else {
            return Err(PlanError::NotAnObject);
        };

        let ir_version = match map.get("ir_version") {
            None | Some(Value::Null) => IR_VERSION,
            Some(v) => {
                let found = v.as_u64().ok_or(PlanError::WrongType {
                    field: "ir_version".to_string(),
                    expected: "an integer",
                })?;
                if found != IR_VERSION {
                    return Err(PlanError::UnsupportedVersion { found });
                }
                found
            }
        };

        let plan_id = optional_str(map, "plan_id")?
            .unwrap_or_default()
            .to_string();
        let documents = string_array(map, "documents")?;
        let constraints = string_array(map, "constraints")?;
        let validators = string_array(map, "validators")?;

        let base_revisions = match map.get("base_revisions") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(entries)) => {
                let mut out = BTreeMap::new();
                for (path, rev) in entries {
                    let rev = rev.as_str().ok_or_else(|| PlanError::WrongType {
                        field: format!("base_revisions.{path}"),
                        expected: "a revision string",
                    })?;
                    out.insert(path.clone(), rev.to_string());
                }
                out
            }
            Some(_) => {
                return Err(PlanError::WrongType {
                    field: "base_revisions".to_string(),
                    expected: "an object of path -> revision",
                })
            }
        };

        let defaults = match map.get("defaults") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(entries)) => entries.clone(),
            Some(_) => {
                return Err(PlanError::WrongType {
                    field: "defaults".to_string(),
                    expected: "an object",
                })
            }
        };

        let rollback_policy = match optional_str(map, "rollback_policy")? {
            None => RollbackPolicy::default(),
            Some(s) => RollbackPolicy::parse(s).ok_or_else(|| PlanError::UnknownPolicy {
                found: s.to_string(),
            })?,
        };

        let raw_ops = match map.get("ops") {
            Some(Value::Array(items)) => items,
            None => {
                return Err(PlanError::Missing {
                    field: "ops".to_string(),
                })
            }
            Some(_) => {
                return Err(PlanError::WrongType {
                    field: "ops".to_string(),
                    expected: "an array of operations",
                })
            }
        };
        if raw_ops.is_empty() {
            return Err(PlanError::Empty);
        }

        let mut ops = Vec::with_capacity(raw_ops.len());
        let mut seen: Vec<String> = Vec::with_capacity(raw_ops.len());
        for (index, raw) in raw_ops.iter().enumerate() {
            let Some(entry) = raw.as_object() else {
                return Err(PlanError::WrongType {
                    field: format!("ops[{index}]"),
                    expected: "an object",
                });
            };
            let name =
                entry
                    .get("op")
                    .and_then(Value::as_str)
                    .ok_or_else(|| PlanError::Missing {
                        field: format!("ops[{index}].op"),
                    })?;

            // An absent id is normal — most operations are never referred to.
            // One is assigned anyway so that every output has an address, which
            // is what lets a reference be validated rather than merely hoped for.
            let id = match entry.get("id") {
                None | Some(Value::Null) => format!("op{}", index + 1),
                Some(Value::String(s)) => s.clone(),
                Some(_) => {
                    return Err(PlanError::WrongType {
                        field: format!("ops[{index}].id"),
                        expected: "a string",
                    })
                }
            };
            if !is_valid_op_id(&id) {
                return Err(PlanError::BadOpId { id });
            }
            if seen.contains(&id) {
                return Err(PlanError::DuplicateOpId { id });
            }

            let with = match entry.get("with") {
                None | Some(Value::Null) => Value::Object(Map::new()),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => {
                    return Err(PlanError::WrongType {
                        field: format!("ops[{index}].with"),
                        expected: "an object",
                    })
                }
            };

            seen.push(id.clone());
            ops.push(Op {
                id,
                op: name.to_string(),
                with,
            });
        }

        Ok(Self {
            plan_id,
            ir_version,
            documents,
            base_revisions,
            defaults,
            ops,
            constraints,
            validators,
            rollback_policy,
        })
    }

    /// Round-trip the plan back to JSON.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        if !self.plan_id.is_empty() {
            map.insert("plan_id".to_string(), Value::String(self.plan_id.clone()));
        }
        map.insert("ir_version".to_string(), Value::from(self.ir_version));
        if !self.documents.is_empty() {
            map.insert("documents".to_string(), Value::from(self.documents.clone()));
        }
        if !self.base_revisions.is_empty() {
            let revs: Map<String, Value> = self
                .base_revisions
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            map.insert("base_revisions".to_string(), Value::Object(revs));
        }
        if !self.defaults.is_empty() {
            map.insert("defaults".to_string(), Value::Object(self.defaults.clone()));
        }
        map.insert(
            "ops".to_string(),
            Value::Array(
                self.ops
                    .iter()
                    .map(|op| {
                        let mut entry = Map::new();
                        entry.insert("id".to_string(), Value::String(op.id.clone()));
                        entry.insert("op".to_string(), Value::String(op.op.clone()));
                        entry.insert("with".to_string(), op.with.clone());
                        Value::Object(entry)
                    })
                    .collect(),
            ),
        );
        if !self.constraints.is_empty() {
            map.insert(
                "constraints".to_string(),
                Value::from(self.constraints.clone()),
            );
        }
        if !self.validators.is_empty() {
            map.insert(
                "validators".to_string(),
                Value::from(self.validators.clone()),
            );
        }
        map.insert(
            "rollback_policy".to_string(),
            Value::String(self.rollback_policy.as_str().to_string()),
        );
        Value::Object(map)
    }
}

fn optional_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, PlanError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(PlanError::WrongType {
            field: key.to_string(),
            expected: "a string",
        }),
    }
}

fn string_array(map: &Map<String, Value>, key: &str) -> Result<Vec<String>, PlanError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| PlanError::WrongType {
                        field: format!("{key}[{i}]"),
                        expected: "a string",
                    })
            })
            .collect(),
        Some(_) => Err(PlanError::WrongType {
            field: key.to_string(),
            expected: "an array of strings",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_minimal_plan_parses() {
        let plan = Plan::from_json(&json!({"ops": [{"op": "call"}]})).unwrap();
        assert_eq!(plan.ops.len(), 1);
        assert_eq!(plan.ops[0].op, "call");
        assert_eq!(plan.ops[0].with, json!({}));
        assert_eq!(plan.rollback_policy, RollbackPolicy::Atomic);
    }

    #[test]
    fn an_operation_without_an_id_still_gets_an_address() {
        // Otherwise a reference has nothing to point at, and "you cannot refer
        // to that one" would depend on whether the author happened to name it.
        let plan =
            Plan::from_json(&json!({"ops": [{"op": "a"}, {"op": "b"}, {"id": "x", "op": "c"}]}))
                .unwrap();
        let ids: Vec<&str> = plan.ops.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["op1", "op2", "x"]);
    }

    #[test]
    fn duplicate_ids_are_refused_rather_than_last_one_wins() {
        let err =
            Plan::from_json(&json!({"ops": [{"id": "a", "op": "x"}, {"id": "a", "op": "y"}]}))
                .unwrap_err();
        assert_eq!(err.code(), "plan_duplicate_op_id");
    }

    #[test]
    fn an_id_that_would_break_a_reference_is_refused() {
        for bad in ["a.b", "a b", "${x}", ""] {
            let err = Plan::from_json(&json!({"ops": [{"id": bad, "op": "x"}]})).unwrap_err();
            assert_eq!(err.code(), "plan_bad_op_id", "id {bad:?} should be refused");
        }
    }

    #[test]
    fn a_future_dialect_is_refused_not_guessed_at() {
        let err = Plan::from_json(&json!({"ir_version": 2, "ops": [{"op": "x"}]})).unwrap_err();
        assert_eq!(err.code(), "plan_unsupported_version");
    }

    #[test]
    fn an_empty_plan_is_an_error_not_a_no_op() {
        assert_eq!(
            Plan::from_json(&json!({"ops": []})).unwrap_err().code(),
            "plan_empty"
        );
        assert_eq!(
            Plan::from_json(&json!({})).unwrap_err().code(),
            "plan_missing_field"
        );
    }

    #[test]
    fn errors_name_the_field_so_a_retry_can_be_targeted() {
        let err = Plan::from_json(&json!({"ops": [{"op": "x", "with": 3}]})).unwrap_err();
        assert_eq!(err.field(), Some("ops[0].with"));
    }

    #[test]
    fn an_unknown_rollback_policy_is_refused() {
        let err = Plan::from_json(&json!({"ops": [{"op": "x"}], "rollback_policy": "maybe"}))
            .unwrap_err();
        assert_eq!(err.code(), "plan_unknown_policy");
    }

    #[test]
    fn a_non_object_defaults_is_refused_before_any_mutation() {
        for bad in [json!("schema.kicad_sch"), json!(3), json!([1, 2])] {
            let err = Plan::from_json(&json!({"ops": [{"op": "x"}], "defaults": bad})).unwrap_err();
            assert_eq!(err.code(), "plan_wrong_type");
            assert_eq!(err.field(), Some("defaults"));
        }
    }

    #[test]
    fn defaults_round_trip() {
        let source = json!({
            "ops": [{"op": "call"}],
            "defaults": {"schematic": "/p/a.kicad_sch"}
        });
        let plan = Plan::from_json(&source).unwrap();
        assert_eq!(plan.defaults["schematic"], json!("/p/a.kicad_sch"));
        let again = Plan::from_json(&plan.to_json()).unwrap();
        assert_eq!(plan, again);
    }

    #[test]
    fn a_plan_round_trips() {
        let source = json!({
            "plan_id": "plan_89",
            "documents": ["/p/a.kicad_sch"],
            "base_revisions": {"a.kicad_sch": "abc"},
            "ops": [{"id": "one", "op": "call", "with": {"tool": "run_erc"}}],
            "constraints": ["RF untouched"],
            "validators": ["erc"],
            "rollback_policy": "continue"
        });
        let plan = Plan::from_json(&source).unwrap();
        let again = Plan::from_json(&plan.to_json()).unwrap();
        assert_eq!(plan, again);
        assert_eq!(again.rollback_policy, RollbackPolicy::Continue);
        assert_eq!(again.base_revisions["a.kicad_sch"], "abc");
    }

    #[test]
    fn policy_answers_the_two_questions_it_is_asked() {
        assert!(RollbackPolicy::Atomic.rolls_back() && RollbackPolicy::Atomic.halts());
        assert!(!RollbackPolicy::StopOnError.rolls_back() && RollbackPolicy::StopOnError.halts());
        assert!(!RollbackPolicy::Continue.rolls_back() && !RollbackPolicy::Continue.halts());
    }
}
