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

use crate::mcp::error::{extract_error_kind, ToolErrorKind};
use crate::mcp::protocol::CallToolResult;
use crate::observability::{new_call_id, unix_ms, CallRecord, CallStatus};
use crate::plan::KicadOps;
use crate::tool;
use crate::tools::{ToolContext, ToolDef};
use kam_plan::{compile, execute::Next, Execution, Plan};
use serde_json::{json, Value};

/// Shared description of the plan document, so the two schemas cannot drift.
const PLAN_DESCRIPTION: &str = "A plan document: {ops: [{op, with, id?}], plan_id?, documents?, \
     rollback_policy?}. Operations: call{tool,args} runs any tool verbatim; \
     place{schematic,components,at?,pitch?,direction?} places symbols, snapped to the \
     1.27mm grid, in one call; power{schematic,symbols:[{net,x,y}]} and \
     label{schematic,labels:[{net,x,y}]} add snapped power symbols and net labels; \
     wire{schematic,segments,junctions?} adds snapped segments; \
     connect{schematic,connections:[{from:'R1.2',to:'R2.1'}]} wires pins; \
     decouple{schematic,ic,at,caps:[{reference,pin|rail,value?}],pitch?,ground?} places a \
     capacitor bank, wires each to its IC pin and grounds it. A later operation may read an \
     earlier one's output with ${op_id.field}.";

pub fn tools() -> Vec<ToolDef> {
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
                    "plan": { "type": "object", "description": PLAN_DESCRIPTION },
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
                    "plan": { "type": "object", "description": PLAN_DESCRIPTION },
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

    let plan = Plan::from_json(document).map_err(|e| {
        CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: e.field().unwrap_or("plan").to_string(),
                reason: e.to_string(),
            },
            format!("The plan was refused before anything ran: {e}"),
        )
    })?;

    compile(&plan, &KicadOps).map_err(|e| {
        CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: e.op_id().unwrap_or("ops").to_string(),
                reason: e.to_string(),
            },
            format!("The plan did not compile, so nothing was applied: {e}"),
        )
    })
}

async fn handle_preview_plan(args: &Value, _ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let program = match build(args) {
        Ok(program) => program,
        Err(rejection) => return Ok(rejection),
    };

    let mut body = program.summary();
    body["references"] = json!(program.has_references());
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
    Ok(CallToolResult::json(&body))
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
        for name in crate::plan::OP_NAMES {
            assert!(
                PLAN_DESCRIPTION.contains(name),
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
}
