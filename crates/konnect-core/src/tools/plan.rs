//! `plan` toolset — describe a change once, run it without being asked again.
//!
//! Two tools, and the pair matters more than either half:
//!
//! * `preview_plan` compiles and returns the exact tool calls, changing
//!   nothing. It is the audit path for a plan: the same compile that would run
//!   produces the listing, so what is reviewed is what executes.
//! * `apply_plan` compiles and runs them.
//!
//! ## Why this is a toolset and not a gateway verb
//!
//! Same argument as the task tools (D20), and it applies more strongly here:
//! `apply_plan`'s schema carries an entire plan document, which would be the
//! single most expensive property in the startup catalogue. As a registry
//! toolset it costs **nothing** until used — `find_capabilities` finds it and
//! `kicad_invoke` calls it without a catalogue refresh.
//!
//! ## Why `apply_plan` runs the steps itself
//!
//! The alternative — compile, hand the calls back, let the caller pass them to
//! `kicad_invoke` — costs a round trip *and* makes the caller pay tokens for
//! the full expansion, which is the cost a plan exists to remove. Running
//! inside `kicad_invoke` instead means the plan inherits everything that batch
//! already guarantees: the snapshot, the rollback, the semantic diff, the
//! `verify` verdict and the task filing. A plan is one MCP call, and it is
//! still a transaction.
//!
//! Every inner step is written to the observability log with its own call id,
//! for the same reason: a plan that mutated a design must not be a mutation
//! without an audit record.

use crate::evidence::validators::{self, Postcondition};
use crate::mcp::error::{extract_error_kind, ToolErrorKind};
use crate::mcp::protocol::CallToolResult;
use crate::observability::{new_call_id, unix_ms, CallRecord, CallStatus};
use crate::plan::KicadOps;
use crate::tool;
use crate::tools::{ToolContext, ToolDef};
use kam_evidence::FindingDelta;
use kam_plan::{compile, execute::Next, Execution, Plan, Program};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn tools() -> Vec<ToolDef> {
    // Shared description of the plan document, so the two schemas cannot
    // drift (one call, reused below) — assembled by `crate::plan::plan_description`
    // from `crate::plan::OP_LIBRARY`, one signature constant per operation
    // declared next to that operation's expander in `plan/ops.rs`, rather than
    // retyped here. That pairing is what keeps this text from promising less
    // than an expander requires: `plan::ops`'s own tests compile an example
    // built from each signature, so a field an expander starts requiring
    // without this text mentioning it fails a test in that module, not
    // silently in a caller's hands.
    let plan_description = crate::plan::plan_description();
    vec![
        tool!(
            "preview_plan",
            "Compile a plan and return the exact tool calls it would run, changing nothing. \
             Use this to check an expansion — how many symbols a decouple actually places, \
             where they land — before applying it. A plan that cannot compile is reported \
             here rather than half-applied.",
            json!({
                "type": "object",
                "properties": {
                    "plan": { "type": "object", "description": plan_description.clone() },
                    "detail": {
                        "type": "string",
                        "enum": ["summary", "calls"],
                        "description": "summary counts the steps per operation; calls lists every one in full. Default summary."
                    }
                },
                "required": ["plan"]
            }),
            |args, ctx| async move { handle_preview_plan(args, ctx).await }
        ),
        // Built by hand rather than through `tool!`: the macro hands the
        // handler a `&ToolContext`, and this one dispatches other tools, whose
        // handlers take the `Arc` by value.
        ToolDef {
            name: "apply_plan",
            description: "Compile a plan and run it. One operation may expand to many tool calls \
                 and a later operation may use an earlier one's result, so a whole design step \
                 costs one call instead of a loop. Call this through kicad_invoke so the plan \
                 inherits the batch's snapshot, rollback, diff and verify. References are checked \
                 before the first mutation: a plan that cannot finish never starts.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "plan": { "type": "object", "description": plan_description },
                    "detail": {
                        "type": "string",
                        "enum": ["summary", "steps"],
                        "description": "summary reports counts and the first failure; steps adds a line per step. Default summary."
                    }
                },
                "required": ["plan"]
            }),
            handler: std::sync::Arc::new(|args, ctx| {
                let args = args.clone();
                Box::pin(async move { handle_apply_plan(&args, ctx).await })
            }),
        },
    ]
}

/// Parse and compile, or return the rejection the caller should act on.
fn build(args: &Value) -> Result<kam_plan::Program, CallToolResult> {
    let Some(document) = args.get("plan") else {
        return Err(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "plan".to_string(),
                reason: "required".to_string(),
            },
            "apply_plan needs a plan: {\"ops\": [{\"op\": \"...\", \"with\": {...}}]}",
        ));
    };

    let mut plan = Plan::from_json(document).map_err(|e| {
        CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: e.field().unwrap_or("plan").to_string(),
                reason: e.to_string(),
            },
            format!("The plan was refused before anything ran: {e}"),
        )
    })?;

    // E24: a `schematic` an operation omitted is filled in with the
    // `${op_id.schematic}` reference a model should have written, but only
    // when exactly one earlier `create` makes the ambiguity go away. See
    // `crate::plan::infer_omitted_schematics` for why this is safe to do
    // before `compile` rather than resolving a path here.
    crate::plan::infer_omitted_schematics(&mut plan);

    let program = compile(&plan, &KicadOps).map_err(|e| {
        CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: e.op_id().unwrap_or("ops").to_string(),
                reason: e.to_string(),
            },
            format!("The plan did not compile, so nothing was applied: {e}"),
        )
    })?;

    // Checked here, before the first mutation, and even for a plan that will
    // only ever be previewed: `kam-plan` carries `validators` as opaque names
    // (D11 — it must stay ignorant of KiCAD), so this is the one place that
    // knows `erc_clean` from a typo. A validator that silently never runs
    // because its name was misspelled is worse than a rejection (D17).
    if let Some(bad) = program
        .validators
        .iter()
        .find(|name| Postcondition::parse(name).is_none())
    {
        return Err(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "validators".to_string(),
                reason: format!(
                    "'{bad}' is not a known validator; use erc, drc, erc_clean or drc_clean"
                ),
            },
            format!(
                "The plan named an unknown validator '{bad}', so nothing was applied. \
                 Known validators: erc, drc, erc_clean, drc_clean."
            ),
        ));
    }

    Ok(program)
}

/// Documents the plan's steps mention, in first-seen order: every string
/// argument ending `.kicad_sch` or `.kicad_pcb`, deduplicated.
///
/// This is advisory the same way `Plan::documents` is (kam-plan's own docs):
/// good enough to know which files a postcondition is about, without needing
/// to understand any one operation's argument shape.
fn touched_documents(program: &Program) -> Vec<PathBuf> {
    let mut docs = Vec::new();
    for step in &program.steps {
        collect_doc_paths(&step.args, &mut docs);
    }
    docs
}

fn collect_doc_paths(value: &Value, out: &mut Vec<PathBuf>) {
    match value {
        Value::String(s) if s.ends_with(".kicad_sch") || s.ends_with(".kicad_pcb") => {
            let path = PathBuf::from(s);
            if !out.contains(&path) {
                out.push(path);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_doc_paths(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_doc_paths(item, out);
            }
        }
        _ => {}
    }
}

/// The name a reply uses for a document: its filename, or the full path when
/// it somehow has none — mirrors `report_validators`.
fn doc_label(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned()
}

async fn handle_preview_plan(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let program = match build(args) {
        Ok(program) => program,
        Err(rejection) => return Ok(rejection),
    };

    let mut body = program.summary();
    body["references"] = json!(program.has_references());
    if !program.validators.is_empty() {
        // Names only, no cache lookups, no `kicad-cli` — preview changes
        // nothing (D6), so this reports what would run, not what it found.
        let docs = touched_documents(&program);
        body["validators_plan"] = json!(program
            .validators
            .iter()
            .filter_map(|name| Postcondition::parse(name))
            .map(|pc| {
                let matching: Vec<String> = docs
                    .iter()
                    .filter(|d| validators::Check::of_path(d) == Some(pc.check()))
                    .map(|d| doc_label(d))
                    .collect();
                json!({ "validator": pc.name(), "documents": matching })
            })
            .collect::<Vec<_>>());
    }
    if args["detail"].as_str() == Some("calls") {
        body["calls"] = program.to_calls();
    }
    Ok(CallToolResult::json(&body))
}

async fn handle_apply_plan(
    args: &Value,
    ctx: std::sync::Arc<ToolContext>,
) -> anyhow::Result<CallToolResult> {
    let program = match build(args) {
        Ok(program) => program,
        Err(rejection) => return Ok(rejection),
    };
    let detail = args["detail"].as_str() == Some("steps");

    // The postconditions the plan is held to. Empty in the overwhelmingly
    // common case, and then this costs nothing: no path is collected, no file
    // is hashed, no `kicad-cli` is spawned (D7).
    let postconditions: Vec<Postcondition> = program
        .validators
        .iter()
        .filter_map(|name| Postcondition::parse(name))
        .collect();
    let docs = if postconditions.is_empty() {
        Vec::new()
    } else {
        touched_documents(&program)
    };
    let steps_total = program.steps.len();

    // Baseline, taken before the first step runs. Only the delta forms
    // (`erc`/`drc`) need one; `_clean` is compared against nothing (D1/D3). A
    // document with no verdict cached for its current revision is validated
    // right now — a plan that asks to be held to a promise pays for it (D3).
    let mut baselines: HashMap<PathBuf, kam_evidence::FindingSet> = HashMap::new();
    for doc in &docs {
        let Some(check) = validators::Check::of_path(doc) else {
            continue;
        };
        let needed = postconditions
            .iter()
            .any(|pc| pc.check() == check && !pc.is_clean());
        if !needed {
            continue;
        }
        let state = kam_state::DocState::read(doc)?;
        let set = match &state {
            // Nothing there yet: an empty baseline, not an unknown one — there
            // was nothing wrong with a document that did not exist (D3).
            kam_state::DocState::Absent => kam_evidence::FindingSet::new(),
            kam_state::DocState::Present(_) => {
                let token = state.token();
                if let Some(cached) = ctx.validation.get(doc, &token) {
                    cached
                } else {
                    match validators::run(check, &ctx.config.kicad_cli, doc).await {
                        Ok(set) => {
                            ctx.validation.put(doc, &token, set.clone());
                            set
                        }
                        Err(e) => {
                            // Could not run before anything was even applied —
                            // a validator that cannot run is a failed
                            // postcondition, never a silent pass (D5).
                            return Ok(postcondition_failure(
                                &json!({ "steps": steps_total, "ok": 0 }),
                                check.name(),
                                &doc_label(doc),
                                0,
                                0,
                                Vec::new(),
                                Some(e.to_string()),
                            ));
                        }
                    }
                }
            }
        };
        baselines.insert(doc.clone(), set);
    }

    let mut run = Execution::new(program);
    loop {
        let step = match run.next_step() {
            Next::Step(step) => step,
            // Already recorded by the executor, with the reference named.
            Next::Unresolved(_) => continue,
            Next::Done => break,
        };

        let Some(def) = ctx.router.find_tool_def(&step.tool) else {
            run.record(
                false,
                json!({
                    "error_kind": "unknown_tool",
                    "error": format!("Tool '{}' does not exist.", step.tool),
                }),
            );
            continue;
        };

        let call_id = new_call_id();
        let ts = unix_ms();
        let started = std::time::Instant::now();
        let args_bytes = serde_json::to_string(&step.args)
            .map(|s| s.len())
            .unwrap_or(0);

        let (ok, output, status, error_kind, result_bytes) =
            match (def.handler)(&step.args, ctx.clone()).await {
                Ok(result) => {
                    let text = result_text(&result);
                    let bytes = text.len();
                    let error_kind = extract_error_kind(&result);
                    let ok = !result.is_error;
                    let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
                    (
                        ok,
                        body,
                        if ok {
                            CallStatus::Ok
                        } else {
                            CallStatus::Error
                        },
                        error_kind,
                        bytes,
                    )
                }
                Err(e) => {
                    let kind = ToolErrorKind::from_anyhow(&e);
                    let short = kind.short_code();
                    (
                        false,
                        json!({
                            "error_kind": short,
                            "transient": kind.transient_class(),
                            "error": e.to_string(),
                        }),
                        CallStatus::Error,
                        Some(short.to_string()),
                        0,
                    )
                }
            };

        ctx.observer
            .record(CallRecord {
                call_id,
                ts,
                tool: step.tool.clone(),
                toolset: ctx
                    .router
                    .find_toolset_for_tool(&step.tool)
                    .map(str::to_string),
                dur_ms: started.elapsed().as_millis() as u64,
                status,
                error_kind,
                args_bytes,
                result_bytes,
            })
            .await;

        run.record(ok, output);
    }

    let report = run.report();
    let mut body = report.to_summary();
    body["ops"] = json!(run.program().expansion.len());
    if let Some(result) = report.results.iter().find(|r| !r.ok) {
        // The failure's own words, not a paraphrase — this is what the caller
        // has to act on, and it is the one thing a summary must not compress.
        body["error"] = result.output["error"]
            .as_str()
            .map_or_else(|| result.output.clone(), |s| json!(s));
        if let Some(kind) = result.output["error_kind"].as_str() {
            body["error_kind"] = json!(kind);
        }
    }
    if detail {
        body["steps_detail"] = json!(report
            .results
            .iter()
            .map(|r| json!({"id": r.id, "op": r.op, "tool": r.tool, "ok": r.ok}))
            .collect::<Vec<_>>());
    }

    // A plan whose steps already failed is already not "done" — its own error
    // is the one the caller must act on, so the postconditions it declared are
    // not evaluated on top of it.
    if report.failed_at.is_none() && !postconditions.is_empty() {
        let mut afters: HashMap<PathBuf, kam_evidence::FindingSet> = HashMap::new();
        for pc in &postconditions {
            for doc in docs
                .iter()
                .filter(|d| validators::Check::of_path(d) == Some(pc.check()))
            {
                let after = match afters.get(doc) {
                    Some(set) => set.clone(),
                    None => {
                        let state = kam_state::DocState::read(doc)?;
                        let set = match &state {
                            kam_state::DocState::Absent => kam_evidence::FindingSet::new(),
                            kam_state::DocState::Present(_) => {
                                let token = state.token();
                                if let Some(cached) = ctx.validation.get(doc, &token) {
                                    cached
                                } else {
                                    match validators::run(pc.check(), &ctx.config.kicad_cli, doc)
                                        .await
                                    {
                                        Ok(set) => {
                                            ctx.validation.put(doc, &token, set.clone());
                                            set
                                        }
                                        Err(e) => {
                                            return Ok(postcondition_failure(
                                                &body,
                                                pc.name(),
                                                &doc_label(doc),
                                                0,
                                                0,
                                                Vec::new(),
                                                Some(e.to_string()),
                                            ));
                                        }
                                    }
                                }
                            }
                        };
                        afters.insert(doc.clone(), set.clone());
                        set
                    }
                };

                let counts = after.counts();
                let (failed, introduced) = if pc.is_clean() {
                    let error_ids: Vec<String> = after
                        .findings
                        .iter()
                        .filter(|f| f.severity == kam_evidence::Severity::Error)
                        .map(|f| f.id.clone())
                        .collect();
                    (counts.errors > 0, error_ids)
                } else {
                    let baseline = baselines.get(doc).cloned().unwrap_or_default();
                    let delta = FindingDelta::between(&baseline, &after);
                    let introduced = delta.introduced.clone();
                    (!introduced.is_empty(), introduced)
                };

                if failed {
                    return Ok(postcondition_failure(
                        &body,
                        pc.name(),
                        &doc_label(doc),
                        counts.errors,
                        counts.warnings,
                        introduced,
                        None,
                    ));
                }
            }
        }
    }

    // A step failure is a failure of the call, not a footnote inside a
    // success: the batch that dispatched us (or an atomic kicad_invoke
    // wrapping us) has to see `is_error` to roll back, exactly as it already
    // does for a failed postcondition above (D28). The body — including
    // `failed_at`, `rollback`, and the step's own `error` — is unchanged.
    Ok(CallToolResult {
        content: vec![crate::mcp::protocol::ToolContent::Text {
            text: body.to_string(),
        }],
        is_error: report.failed_at.is_some(),
    })
}

/// Build the `postcondition_failed` result: the structured error, with the
/// steps-so-far summary folded in so a failure never hides what actually ran
/// (D4 — the summary must survive the failure it explains).
fn postcondition_failure(
    summary: &Value,
    check: &str,
    document: &str,
    errors: usize,
    warnings: usize,
    introduced: Vec<String>,
    reason: Option<String>,
) -> CallToolResult {
    let message = match &reason {
        Some(reason) => format!(
            "Postcondition '{check}' on {document} could not run: {reason}. A validator that \
             cannot run is a failed postcondition, never a clean one."
        ),
        None => format!(
            "Postcondition '{check}' failed on {document}: {errors} error(s), {warnings} \
             warning(s), {} new finding(s).",
            introduced.len()
        ),
    };
    let kind = ToolErrorKind::PostconditionFailed {
        check: check.to_string(),
        document: document.to_string(),
        errors,
        warnings,
        introduced,
        reason,
    };
    let base = CallToolResult::error_kind(kind, message);
    let mut merged: Value = serde_json::from_str(&result_text(&base)).unwrap_or_else(|_| json!({}));
    if let (Some(m), Some(s)) = (merged.as_object_mut(), summary.as_object()) {
        for (k, v) in s {
            m.insert(k.clone(), v.clone());
        }
    }
    CallToolResult {
        content: vec![crate::mcp::protocol::ToolContent::Text {
            text: merged.to_string(),
        }],
        is_error: true,
    }
}

fn result_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            crate::mcp::protocol::ToolContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolset_registers_two_tools() {
        let names: Vec<&str> = tools().iter().map(|t| t.name).collect();
        assert_eq!(names, ["preview_plan", "apply_plan"]);
    }

    #[test]
    fn every_operation_the_library_implements_is_documented_in_the_schema() {
        // An operation nothing describes is an operation no caller can reach,
        // and the description is the only place a gateway client learns the
        // vocabulary — there is no per-operation schema to discover.
        let description = crate::plan::plan_description();
        for name in crate::plan::op_names() {
            assert!(
                description.contains(name),
                "operation '{name}' is implemented but not described"
            );
        }
    }

    #[test]
    fn a_plan_that_cannot_compile_is_refused_with_the_operation_named() {
        let err = build(&json!({"plan": {"ops": [
            {"id": "a", "op": "call", "with": {"tool": "use", "args": {"r": "${b.x}"}}},
            {"id": "b", "op": "call", "with": {"tool": "make"}}
        ]}}))
        .unwrap_err();
        let text = result_text(&err);
        assert!(text.contains("does not run before"), "{text}");
        assert!(text.contains("\"field\":\"a\""), "{text}");
    }

    #[test]
    fn a_malformed_plan_names_its_field() {
        let err = build(&json!({"plan": {"ops": [{"op": "place", "with": 3}]}})).unwrap_err();
        assert!(result_text(&err).contains("ops[0].with"));
    }

    #[test]
    fn a_missing_plan_is_an_invalid_argument_not_a_panic() {
        let err = build(&json!({})).unwrap_err();
        assert!(result_text(&err).contains("invalid_argument"));
    }

    // ─── Postconditions ────────────────────────────────────────────────────

    fn test_config(kicad_cli: &str) -> crate::tools::ServerConfig {
        crate::tools::ServerConfig {
            kicad_cli: kicad_cli.to_string(),
            kicad_binary: String::new(),
            ipc_address: String::new(),
            project_dir: None,
            jlcpcb_db_path: None,
            auto_load_toolsets: false,
            mode: kam_state::OperatingMode::Write,
        }
    }

    async fn context(kicad_cli: &str) -> std::sync::Arc<ToolContext> {
        let router = std::sync::Arc::new(crate::router::ToolRouter::new());
        router.load("plan").await;
        router.load("task").await;
        std::sync::Arc::new(ToolContext::new(test_config(kicad_cli), router))
    }

    #[test]
    fn an_unknown_validator_name_is_refused_by_build() {
        let err = build(&json!({"plan": {
            "ops": [{"op": "call", "with": {"tool": "start_task", "args": {"objective": "x"}}}],
            "validators": ["ercc"]
        }}))
        .unwrap_err();
        let text = result_text(&err);
        assert_eq!(
            extract_error_kind(&err).as_deref(),
            Some("invalid_argument")
        );
        assert!(text.contains("\"field\":\"validators\""), "{text}");
        assert!(text.contains("ercc"), "{text}");
    }

    #[tokio::test]
    async fn an_unknown_validator_name_stops_apply_plan_before_any_step_runs() {
        let ctx = context("no-such-kicad-cli").await;
        let args = json!({"plan": {
            "ops": [{"op": "call", "with": {"tool": "start_task", "args": {"objective": "x"}}}],
            "validators": ["ercc"]
        }});
        let result = handle_apply_plan(&args, ctx.clone()).await.unwrap();
        assert!(result.is_error);
        assert_eq!(
            extract_error_kind(&result).as_deref(),
            Some("invalid_argument")
        );
        assert!(
            ctx.observer.recent(10).await.is_empty(),
            "build() rejects before any step is dispatched, so nothing should be recorded"
        );
    }

    #[tokio::test]
    async fn preview_plan_lists_validators_and_their_documents_without_running_them() {
        // A kicad_cli that cannot possibly run: if preview ever tried to
        // validate, this would surface as an error rather than the listing.
        let ctx = context("no-such-kicad-cli").await;
        let args = json!({"plan": {
            "ops": [{"op": "call", "with": {
                "tool": "start_task",
                "args": {"objective": "x", "schematic": "a.kicad_sch"}
            }}],
            "validators": ["erc"]
        }});
        let result = handle_preview_plan(&args, &ctx).await.unwrap();
        assert!(!result.is_error, "{}", result_text(&result));
        let body: Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(
            body["validators_plan"],
            json!([{"validator": "erc", "documents": ["a.kicad_sch"]}])
        );
    }

    #[tokio::test]
    async fn a_plan_with_no_validators_never_calls_kicad_cli() {
        // The document exists and is a real path, so if the empty-validators
        // fast path ever regressed into computing a baseline anyway, this
        // deliberately-broken kicad_cli would turn that into a visible error.
        let dir = tempfile::tempdir().unwrap();
        let sch = dir.path().join("a.kicad_sch");
        std::fs::write(&sch, "(kicad_sch)").unwrap();

        let ctx = context("definitely-not-a-real-kicad-cli-binary").await;
        let args = json!({"plan": {
            "ops": [{"op": "call", "with": {
                "tool": "start_task",
                "args": {"objective": "x", "schematic": sch.to_string_lossy()}
            }}]
        }});
        let result = handle_apply_plan(&args, ctx).await.unwrap();
        assert!(!result.is_error, "{}", result_text(&result));
    }

    #[tokio::test]
    async fn a_failed_step_sets_is_error_so_an_atomic_batch_rolls_back() {
        // D28 already does this for a failed postcondition; a step that fails
        // outright (here: a tool the router has never heard of) must not look
        // like success to whatever dispatched apply_plan — an atomic
        // kicad_invoke wrapping it decides whether to roll back by reading
        // `is_error` on this very result, not by re-parsing the body (E4).
        let ctx = context("no-such-kicad-cli").await;
        let args = json!({"plan": {
            "ops": [{"op": "call", "with": {"tool": "no_such_tool_xyz", "args": {}}}]
        }});
        let result = handle_apply_plan(&args, ctx).await.unwrap();
        assert!(result.is_error, "{}", result_text(&result));
        let body: Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(body["failed_at"], json!(0));
        assert_eq!(body["ok"], json!(0));
    }
}
