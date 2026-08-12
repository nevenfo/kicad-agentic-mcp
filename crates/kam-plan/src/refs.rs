//! `${op.path}` — how a later operation names an earlier one's output.
//!
//! This is the half of the plan that removes round trips. Without it, an
//! operation that needs the reference designator the previous one assigned has
//! to come back through the model to read it: tool, result, inference, tool.
//! With it, the dependency is written down once and settled by string
//! substitution.
//!
//! The syntax is deliberately small, because it is written by models and read by
//! reviewers:
//!
//! * `"${place.reference}"` — the whole string is one reference, and it resolves
//!   to the *value* at that path. A number stays a number; an object stays an
//!   object. This matters: `{"x": "${p.x}"}` must not turn a coordinate into a
//!   string that the tool below then refuses.
//! * `"R${place.index}"` — a reference inside a larger string interpolates, and
//!   only scalars may do so. Substituting an object into a sentence is never
//!   what was meant, so it is an error rather than `[object Object]`.
//! * `"$${literal}"` — an escaped `${`, for the rare argument that contains one.
//!
//! Paths are dotted, with numeric segments indexing arrays:
//! `${place.items.0.uuid}`. The first segment names either an operation id or
//! one step of a macro (`${decouple/2.uuid}`). The two namespaces cannot
//! collide: an operation id may not contain `/`, so anything that does is a
//! step and anything that does not is an operation.

use serde_json::{Map, Value};

/// One segment of a reference path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// An object key.
    Key(String),
    /// An array index.
    Index(usize),
}

/// A parsed `${op.path}` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Operation, or macro step, whose output is being read.
    pub op: String,
    /// Path into that output. Empty means the output itself.
    pub path: Vec<Segment>,
}

impl std::fmt::Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${{{}", self.op)?;
        for segment in &self.path {
            match segment {
                Segment::Key(k) => write!(f, ".{k}")?,
                Segment::Index(i) => write!(f, ".{i}")?,
            }
        }
        write!(f, "}}")
    }
}

/// Why a reference could not be parsed or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    /// A `${` was opened and never closed.
    Unterminated {
        /// The string it appeared in.
        text: String,
    },
    /// The contents of `${...}` were not a valid `op.path`.
    Malformed {
        /// What was between the braces.
        text: String,
    },
    /// The reference names an operation the plan does not contain, or one that
    /// has not produced an output yet.
    UnknownOp {
        /// The reference as written.
        reference: String,
        /// The operation id it named.
        op: String,
    },
    /// The operation ran, but its output has nothing at that path.
    NoSuchPath {
        /// The reference as written.
        reference: String,
    },
    /// The reference resolved to a composite value inside a larger string.
    NotScalar {
        /// The reference as written.
        reference: String,
    },
}

impl RefError {
    /// Stable code for matching.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unterminated { .. } => "ref_unterminated",
            Self::Malformed { .. } => "ref_malformed",
            Self::UnknownOp { .. } => "ref_unknown_op",
            Self::NoSuchPath { .. } => "ref_no_such_path",
            Self::NotScalar { .. } => "ref_not_scalar",
        }
    }
}

impl std::fmt::Display for RefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unterminated { text } => {
                write!(f, "'{text}' opens a ${{ reference and never closes it")
            }
            Self::Malformed { text } => write!(
                f,
                "'${{{text}}}' is not a reference; expected ${{op}} or ${{op.field.0}}"
            ),
            Self::UnknownOp { reference, op } => write!(
                f,
                "{reference} refers to '{op}', which is not an earlier operation in this plan"
            ),
            Self::NoSuchPath { reference } => {
                write!(
                    f,
                    "{reference} has no such field in that operation's output"
                )
            }
            Self::NotScalar { reference } => write!(
                f,
                "{reference} resolves to an object or array, which cannot be embedded in a string"
            ),
        }
    }
}

impl std::error::Error for RefError {}

/// A string after parsing: literal text, one whole reference, or a mix.
#[derive(Debug, Clone, PartialEq)]
enum Parsed {
    Literal(String),
    Whole(Reference),
    Mixed(Vec<Piece>),
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Text(String),
    Ref(Reference),
}

fn parse_string(input: &str) -> Result<Parsed, RefError> {
    if !input.contains("${") && !input.contains("$$") {
        return Ok(Parsed::Literal(input.to_string()));
    }

    let mut pieces: Vec<Piece> = Vec::new();
    let mut literal = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 2 < bytes.len() && bytes[i + 1] == b'$' && bytes[i + 2] == b'{' {
            literal.push_str("${");
            i += 3;
            continue;
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let Some(end) = input[i + 2..].find('}').map(|off| i + 2 + off) else {
                return Err(RefError::Unterminated {
                    text: input.to_string(),
                });
            };
            let body = &input[i + 2..end];
            let reference = parse_reference(body)?;
            if !literal.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut literal)));
            }
            pieces.push(Piece::Ref(reference));
            i = end + 1;
            continue;
        }
        let ch = input[i..].chars().next().unwrap_or('\0');
        literal.push(ch);
        i += ch.len_utf8();
    }

    if !literal.is_empty() {
        pieces.push(Piece::Text(literal));
    }

    if pieces.len() > 1 {
        return Ok(Parsed::Mixed(pieces));
    }
    match pieces.pop() {
        None => Ok(Parsed::Literal(String::new())),
        Some(Piece::Text(t)) => Ok(Parsed::Literal(t)),
        Some(Piece::Ref(r)) => Ok(Parsed::Whole(r)),
    }
}

/// Whether a string can be the target of a reference — an operation id, or one
/// step of a macro such as `decouple/2`.
///
/// Looser than [`crate::ir::is_valid_op_id`] by exactly one character, `/`, and
/// that is what keeps the two namespaces disjoint rather than merely
/// conventionally separate.
#[must_use]
pub fn is_valid_ref_target(target: &str) -> bool {
    !target.is_empty()
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/')
}

/// The zero-based position an `ops[N]` target names, if `target` is written
/// that way — `${ops[0].field}` for the first operation in the plan,
/// regardless of its id. Never collides with an ordinary id: `[` and `]`
/// cannot appear in one ([`is_valid_ref_target`]), so this shape is only ever
/// this.
#[must_use]
pub fn position_target(target: &str) -> Option<usize> {
    let digits = target.strip_prefix("ops[")?.strip_suffix(']')?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()
}

fn parse_reference(body: &str) -> Result<Reference, RefError> {
    let malformed = || RefError::Malformed {
        text: body.to_string(),
    };
    let mut parts = body.split('.');
    let op = parts.next().ok_or_else(malformed)?;
    if !is_valid_ref_target(op) && position_target(op).is_none() {
        return Err(malformed());
    }
    let mut path = Vec::new();
    for part in parts {
        if part.is_empty() {
            return Err(malformed());
        }
        if part.chars().all(|c| c.is_ascii_digit()) {
            let index = part.parse::<usize>().map_err(|_| malformed())?;
            path.push(Segment::Index(index));
        } else {
            path.push(Segment::Key(part.to_string()));
        }
    }
    Ok(Reference {
        op: op.to_string(),
        path,
    })
}

/// Every reference appearing anywhere in a JSON value, in document order.
///
/// # Errors
///
/// Returns the first malformed reference. A plan with a typo in a reference is
/// refused at compile time rather than at the step that would have used it,
/// because by then part of the plan has already been applied.
pub fn scan(value: &Value) -> Result<Vec<Reference>, RefError> {
    let mut out = Vec::new();
    scan_into(value, &mut out)?;
    Ok(out)
}

fn scan_into(value: &Value, out: &mut Vec<Reference>) -> Result<(), RefError> {
    match value {
        Value::String(s) => match parse_string(s)? {
            Parsed::Literal(_) => {}
            Parsed::Whole(r) => out.push(r),
            Parsed::Mixed(pieces) => {
                for piece in pieces {
                    if let Piece::Ref(r) = piece {
                        out.push(r);
                    }
                }
            }
        },
        Value::Array(items) => {
            for item in items {
                scan_into(item, out)?;
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                scan_into(item, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Substitute every reference in `value` from `outputs`.
///
/// # Errors
///
/// Returns the first reference that names an absent operation, an absent path,
/// or a composite value used inside a larger string.
pub fn resolve(value: &Value, outputs: &dyn Outputs) -> Result<Value, RefError> {
    match value {
        Value::String(s) => match parse_string(s)? {
            Parsed::Literal(t) => Ok(Value::String(t)),
            Parsed::Whole(r) => lookup(&r, outputs),
            Parsed::Mixed(pieces) => {
                let mut out = String::new();
                for piece in pieces {
                    match piece {
                        Piece::Text(t) => out.push_str(&t),
                        Piece::Ref(r) => {
                            let found = lookup(&r, outputs)?;
                            match found {
                                Value::String(s) => out.push_str(&s),
                                Value::Number(n) => out.push_str(&n.to_string()),
                                Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
                                Value::Null => out.push_str("null"),
                                _ => {
                                    return Err(RefError::NotScalar {
                                        reference: r.to_string(),
                                    })
                                }
                            }
                        }
                    }
                }
                Ok(Value::String(out))
            }
        },
        Value::Array(items) => items
            .iter()
            .map(|item| resolve(item, outputs))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), resolve(v, outputs)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// Why [`rewrite`] could not finish: a reference did not parse, or the
/// caller's `resolve` refused the target it named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteError<E> {
    /// A `${...}` in `value` did not parse. Same failure [`resolve`] would
    /// have reported, at the same place: before anything runs.
    Ref(RefError),
    /// `resolve` refused a reference's target — an ambiguous one, typically,
    /// since an unparseable or absent one is [`Self::Ref`] or left for the
    /// caller to detect once every target is canonical.
    Target(E),
}

/// Rewrite every reference's *target* in `value` — the `op` a reference
/// names — leaving its path and every surrounding character untouched.
///
/// This is how a tolerant way of writing a reference (`${ops[0].field}`, or
/// `${create.field}` when exactly one operation is a `create`) turns into the
/// one canonical form — `${op_id.field}` — that the rest of a compiled plan,
/// and everything at run time, ever has to understand. Nothing downstream of
/// compilation needs to know the tolerant forms exist.
///
/// # Errors
///
/// The first reference that fails to parse, or whose target `resolve`
/// refuses. Both are returned before `value` is used for anything, matching
/// [`resolve`] and [`scan`].
pub fn rewrite<E>(
    value: &Value,
    resolve_target: &mut dyn FnMut(&Reference) -> Result<String, E>,
) -> Result<Value, RewriteError<E>> {
    match value {
        Value::String(s) => match parse_string(s).map_err(RewriteError::Ref)? {
            Parsed::Literal(t) => Ok(Value::String(t)),
            Parsed::Whole(r) => {
                let target = resolve_target(&r).map_err(RewriteError::Target)?;
                Ok(Value::String(
                    Reference {
                        op: target,
                        path: r.path,
                    }
                    .to_string(),
                ))
            }
            Parsed::Mixed(pieces) => {
                let mut out = String::new();
                for piece in pieces {
                    match piece {
                        Piece::Text(t) => out.push_str(&t),
                        Piece::Ref(r) => {
                            let target = resolve_target(&r).map_err(RewriteError::Target)?;
                            out.push_str(
                                &Reference {
                                    op: target,
                                    path: r.path,
                                }
                                .to_string(),
                            );
                        }
                    }
                }
                Ok(Value::String(out))
            }
        },
        Value::Array(items) => items
            .iter()
            .map(|item| rewrite(item, resolve_target))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), rewrite(v, resolve_target)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn lookup(reference: &Reference, outputs: &dyn Outputs) -> Result<Value, RefError> {
    let Some(root) = outputs.output(&reference.op) else {
        return Err(RefError::UnknownOp {
            reference: reference.to_string(),
            op: reference.op.clone(),
        });
    };
    let mut cursor = root;
    for segment in &reference.path {
        let next = match (segment, &cursor) {
            (Segment::Key(k), Value::Object(map)) => map.get(k).cloned(),
            (Segment::Index(i), Value::Array(items)) => items.get(*i).cloned(),
            // A numeric segment against an object is a legitimate key lookup:
            // `${erc.counts.0}` where the output really does key by "0".
            (Segment::Index(i), Value::Object(map)) => map.get(&i.to_string()).cloned(),
            _ => None,
        };
        cursor = next.ok_or_else(|| RefError::NoSuchPath {
            reference: reference.to_string(),
        })?;
    }
    Ok(cursor)
}

/// Where resolved outputs come from.
///
/// A trait rather than a map so the executor can answer from whatever it keeps
/// — including refusing an operation that has not run yet, which a plain map
/// cannot express.
pub trait Outputs {
    /// The output recorded for an operation, if it has one.
    fn output(&self, op: &str) -> Option<Value>;
}

impl Outputs for std::collections::BTreeMap<String, Value> {
    fn output(&self, op: &str) -> Option<Value> {
        self.get(op).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn outputs() -> BTreeMap<String, Value> {
        let mut map = BTreeMap::new();
        map.insert(
            "place".to_string(),
            json!({"reference": "R1", "x": 100.33, "pins": [{"uuid": "abc"}, {"uuid": "def"}]}),
        );
        map.insert("count".to_string(), json!(3));
        map
    }

    #[test]
    fn a_whole_string_reference_keeps_the_value_s_type() {
        // The point of the distinction: a coordinate must arrive as a number.
        let resolved = resolve(&json!({"x": "${place.x}"}), &outputs()).unwrap();
        assert_eq!(resolved, json!({"x": 100.33}));
        assert!(resolved["x"].is_f64());
    }

    #[test]
    fn an_embedded_reference_interpolates() {
        let resolved = resolve(&json!("net_${place.reference}_out"), &outputs()).unwrap();
        assert_eq!(resolved, json!("net_R1_out"));
    }

    #[test]
    fn a_composite_cannot_be_embedded_in_a_sentence() {
        let err = resolve(&json!("pins are ${place.pins}"), &outputs()).unwrap_err();
        assert_eq!(err.code(), "ref_not_scalar");
    }

    #[test]
    fn a_composite_can_be_a_whole_value() {
        let resolved = resolve(&json!({"pins": "${place.pins}"}), &outputs()).unwrap();
        assert_eq!(resolved["pins"][1]["uuid"], json!("def"));
    }

    #[test]
    fn array_indices_are_paths() {
        let resolved = resolve(&json!("${place.pins.1.uuid}"), &outputs()).unwrap();
        assert_eq!(resolved, json!("def"));
    }

    #[test]
    fn nested_structures_are_resolved_throughout() {
        let resolved = resolve(
            &json!({"a": [{"b": "${place.reference}"}], "c": {"d": "${count}"}}),
            &outputs(),
        )
        .unwrap();
        assert_eq!(resolved, json!({"a": [{"b": "R1"}], "c": {"d": 3}}));
    }

    #[test]
    fn an_escaped_dollar_brace_is_literal() {
        let resolved = resolve(&json!("$${not a ref}"), &outputs()).unwrap();
        assert_eq!(resolved, json!("${not a ref}"));
    }

    #[test]
    fn an_unknown_operation_is_named_in_the_error() {
        let err = resolve(&json!("${ghost.x}"), &outputs()).unwrap_err();
        assert_eq!(err.code(), "ref_unknown_op");
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn a_missing_path_is_distinguished_from_a_missing_operation() {
        let err = resolve(&json!("${place.nope}"), &outputs()).unwrap_err();
        assert_eq!(err.code(), "ref_no_such_path");
    }

    #[test]
    fn an_unterminated_reference_is_refused() {
        let err = resolve(&json!("${place.x"), &outputs()).unwrap_err();
        assert_eq!(err.code(), "ref_unterminated");
    }

    #[test]
    fn a_malformed_body_is_refused() {
        for bad in ["${place..x}", "${pl ace.x}", "${.x}"] {
            let err = resolve(&json!(bad), &outputs()).unwrap_err();
            assert_eq!(err.code(), "ref_malformed", "{bad} should be malformed");
        }
    }

    #[test]
    fn scan_finds_every_reference_without_needing_outputs() {
        let found =
            scan(&json!({"a": "${one.x}", "b": ["${two}", "lit", "x${three.y}z"]})).unwrap();
        let ops: Vec<&str> = found.iter().map(|r| r.op.as_str()).collect();
        assert_eq!(ops, ["one", "two", "three"]);
        assert_eq!(found[1].path, Vec::new());
    }

    #[test]
    fn a_reference_displays_as_it_was_written() {
        let parsed = parse_reference("place.pins.1.uuid").unwrap();
        assert_eq!(parsed.to_string(), "${place.pins.1.uuid}");
    }

    #[test]
    fn a_plain_string_is_untouched() {
        let resolved = resolve(&json!("Device:R"), &outputs()).unwrap();
        assert_eq!(resolved, json!("Device:R"));
    }

    #[test]
    fn a_positional_target_parses_as_ops_bracket_n() {
        assert_eq!(position_target("ops[0]"), Some(0));
        assert_eq!(position_target("ops[12]"), Some(12));
        assert_eq!(position_target("ops[]"), None);
        assert_eq!(position_target("ops[x]"), None);
        assert_eq!(position_target("place"), None);
    }

    #[test]
    fn a_positional_reference_scans_like_any_other() {
        let found = scan(&json!("${ops[0].schematic}")).unwrap();
        assert_eq!(found[0].op, "ops[0]");
        assert_eq!(found[0].path, vec![Segment::Key("schematic".to_string())]);
    }

    #[test]
    fn rewrite_replaces_only_the_target_leaving_the_path_and_surrounding_text() {
        let rewritten = rewrite::<()>(
            &json!({"a": "${ops[0].x}", "b": "net_${ops[0].reference}_out"}),
            &mut |r| {
                assert_eq!(r.op, "ops[0]");
                Ok("place".to_string())
            },
        )
        .unwrap();
        assert_eq!(
            rewritten,
            json!({"a": "${place.x}", "b": "net_${place.reference}_out"})
        );
    }

    #[test]
    fn rewrite_surfaces_a_parse_error_before_resolving_anything() {
        let err = rewrite::<()>(&json!("${place.x"), &mut |_| Ok("x".to_string())).unwrap_err();
        assert!(matches!(
            err,
            RewriteError::Ref(RefError::Unterminated { .. })
        ));
    }

    #[test]
    fn rewrite_surfaces_the_caller_s_own_refusal() {
        let err = rewrite(&json!("${place.x}"), &mut |_| Err("ambiguous")).unwrap_err();
        assert_eq!(err, RewriteError::Target("ambiguous"));
    }
}
