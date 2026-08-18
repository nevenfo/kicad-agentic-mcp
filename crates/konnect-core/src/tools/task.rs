//! `task` toolset — the objective, held outside the model that is pursuing it.
//!
//! Everything else in this server answers "what is the design?". These four
//! tools answer "what am I doing to it?", and they answer it from a record
//! rather than from a transcript, so a compaction, a restart or a model swap
//! does not lose the objective, the constraints or the list of things already
//! tried and known not to work.
//!
//! ## Why this is a registry toolset and not a gateway verb
//!
//! `tools/list` at startup is the most expensive real estate the server owns:
//! every token is re-sent on every catalogue refresh. Four task tools as
//! always-visible meta-tools would cost a few hundred tokens per session to
//! every client, including the ones that never open a task. As a toolset they
//! cost **nothing** until used — `find_capabilities("track this objective")`
//! finds them and `kicad_invoke` calls them without touching the catalogue,
//! which is exactly the case the gateway was built for.
//!
//! The state itself lives in `kam-state::task`, which knows nothing about
//! KiCAD; this module is only the MCP skin on it.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tools::{require_str, ToolContext, ToolDef};
use crate::{tool, try_arg};
use kam_state::task::{TaskError, TaskState, TaskStatus};
use serde_json::{json, Value};

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "start_task",
            "Open a task: an objective, its hard constraints and how success will be judged, \
             kept outside the conversation so a compaction cannot lose them. Returns a task_id \
             and a short ACTIVE TASK anchor to repeat back into later prompts. Pass the id to \
             kicad_invoke(task_id=...) and every batch attaches its revisions and evidence to \
             the task by itself.",
            json!({
                "type": "object",
                "properties": {
                    "objective": { "type": "string", "description": "What success would be, in one sentence" },
                    "constraints": {
                        "type": "array", "items": { "type": "string" },
                        "description": "Must-hold conditions, e.g. 'do not touch the RF section'"
                    },
                    "success_criteria": {
                        "type": "array", "items": { "type": "string" },
                        "description": "How completion is judged, e.g. 'ERC = 0'"
                    },
                    "subgoal": { "type": "string", "description": "The part being worked on first" }
                },
                "required": ["objective"]
            }),
            |args, ctx| async move { handle_start_task(args, ctx).await }
        ),
        tool!(
            "update_task",
            "Add to a task's record: the current subgoal, progress, verified facts, assumptions, \
             unresolved items, an attempt that failed, an approach to stop retrying, or a new \
             status. Everything appends; nothing is replaced except subgoal, progress and status. \
             Returns the refreshed ACTIVE TASK anchor.",
            json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "subgoal": { "type": "string" },
                    "status": {
                        "type": "string",
                        "enum": ["active", "blocked", "needs_review", "done", "abandoned"]
                    },
                    "progress": {
                        "type": "object",
                        "properties": { "done": {"type": "integer"}, "total": {"type": "integer"} },
                        "description": "Sub-goals completed out of expected"
                    },
                    "constraints": { "type": "array", "items": {"type": "string"} },
                    "success_criteria": { "type": "array", "items": {"type": "string"} },
                    "verified_facts": {
                        "type": "array", "items": {"type": "string"},
                        "description": "Established by a validator or a real read — not by inference"
                    },
                    "assumptions": { "type": "array", "items": {"type": "string"} },
                    "unresolved": { "type": "array", "items": {"type": "string"} },
                    "forbidden_repeats": { "type": "array", "items": {"type": "string"} },
                    "failed": {
                        "type": "object",
                        "properties": { "what": {"type": "string"}, "why": {"type": "string"} },
                        "description": "An attempt that did not work, and the failure's own words"
                    }
                },
                "required": ["task_id"]
            }),
            |args, ctx| async move { handle_update_task(args, ctx).await }
        ),
        tool!(
            "get_task",
            "Return a task's full record: objective, constraints, success criteria, verified \
             facts, assumptions, unresolved items, failed attempts, document revisions, evidence \
             handles and progress — plus the ACTIVE TASK anchor. This is what to read after a \
             compaction or when picking work back up.",
            json!({
                "type": "object",
                "properties": { "task_id": { "type": "string" } },
                "required": ["task_id"]
            }),
            |args, ctx| async move { handle_get_task(args, ctx).await }
        ),
        tool!(
            "list_tasks",
            "List the tasks this session knows about — id, truncated objective, status and \
             progress. Use get_task for the detail.",
            json!({ "type": "object", "properties": {}, "required": [] }),
            |args, ctx| async move { handle_list_tasks(args, ctx).await }
        ),
    ]
}

async fn handle_start_task(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let objective = try_arg!(require_str(args, "objective"));
    let task = ctx.tasks.start(objective);

    let (task, refused) = match ctx.tasks.update(&task.id, |t| {
        if let Some(subgoal) = args["subgoal"].as_str() {
            t.subgoal = Some(subgoal.to_string());
        }
        Ok(apply_lists(t, args))
    }) {
        Ok((task, refused)) => (task, refused),
        // The task was created a line ago; it cannot have been evicted.
        Err(e) => return Ok(task_error(&e)),
    };

    Ok(reply(&task, refused, None))
}

async fn handle_update_task(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let task_id = try_arg!(require_str(args, "task_id"));

    let status = match args["status"].as_str() {
        None => None,
        Some(s) => match parse_status(s) {
            Some(status) => Some(status),
            None => {
                let kind = ToolErrorKind::InvalidArgument {
                    field: "status".to_string(),
                    reason: format!("unknown status '{s}'"),
                };
                return Ok(CallToolResult::error_kind(
                    kind,
                    "status must be one of: active, blocked, needs_review, done, abandoned",
                ));
            }
        },
    };

    let updated = ctx.tasks.update(task_id, |t| {
        if let Some(subgoal) = args["subgoal"].as_str() {
            t.subgoal = Some(subgoal.to_string());
        }
        if let Some(status) = status {
            t.status = status;
        }
        if let Some(progress) = args.get("progress") {
            if let Some(done) = progress["done"].as_u64() {
                t.progress.done = done as usize;
            }
            if let Some(total) = progress["total"].as_u64() {
                t.progress.total = total as usize;
            }
        }
        if let Some(failed) = args.get("failed") {
            if let Some(what) = failed["what"].as_str() {
                t.add_failed_attempt(what, failed["why"].as_str().unwrap_or("unspecified"));
            }
        }
        Ok(apply_lists(t, args))
    });

    match updated {
        Ok((task, refused)) => Ok(reply(&task, refused, None)),
        Err(e) => Ok(task_error(&e)),
    }
}

async fn handle_get_task(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let task_id = try_arg!(require_str(args, "task_id"));
    match ctx.tasks.get(task_id) {
        Some(task) => Ok(reply(&task, Vec::new(), Some(&task))),
        None => Ok(task_error(&TaskError::Unknown(task_id.to_string()))),
    }
}

async fn handle_list_tasks(_args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let tasks = ctx.tasks.list();
    Ok(CallToolResult::json(&json!({
        "count": tasks.len(),
        "tasks": tasks,
    })))
}

/// Append every list argument that is present, collecting the entries a
/// bounded list refused rather than dropping them on the floor.
fn apply_lists(task: &mut TaskState, args: &Value) -> Vec<String> {
    let mut refused = Vec::new();
    for value in strings(args, "constraints") {
        if let Err(e) = task.add_constraint(value.clone()) {
            refused.push(format!("constraints: {value} ({})", e.code()));
        }
    }
    for value in strings(args, "success_criteria") {
        if let Err(e) = task.add_success_criterion(value.clone()) {
            refused.push(format!("success_criteria: {value} ({})", e.code()));
        }
    }
    for value in strings(args, "verified_facts") {
        task.add_verified_fact(value);
    }
    for value in strings(args, "assumptions") {
        task.add_assumption(value);
    }
    for value in strings(args, "unresolved") {
        task.add_unresolved(value);
    }
    for value in strings(args, "forbidden_repeats") {
        task.forbid(value);
    }
    refused
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    args[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_status(s: &str) -> Option<TaskStatus> {
    match s {
        "active" => Some(TaskStatus::Active),
        "blocked" => Some(TaskStatus::Blocked),
        "needs_review" => Some(TaskStatus::NeedsReview),
        "done" => Some(TaskStatus::Done),
        "abandoned" => Some(TaskStatus::Abandoned),
        _ => None,
    }
}

/// The standard reply: the id, the anchor, and the full record only when it
/// was asked for. Returning the whole record on every update would make the
/// task state as expensive as the transcript it replaces.
fn reply(task: &TaskState, refused: Vec<String>, full: Option<&TaskState>) -> CallToolResult {
    let mut body = json!({
        "task_id": task.id,
        "status": task.status,
        "anchor": task.anchor(),
    });
    if !refused.is_empty() {
        // A refused hard constraint must be loud. The caller asked for a
        // guarantee and did not get it.
        body["refused"] = json!(refused);
    }
    if let Some(task) = full {
        body["task"] = serde_json::to_value(task).unwrap_or(Value::Null);
    }
    CallToolResult::json(&body)
}

fn task_error(e: &TaskError) -> CallToolResult {
    CallToolResult::error_kind(
        ToolErrorKind::InvalidArgument {
            field: "task_id".to_string(),
            reason: e.code().to_string(),
        },
        e.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::ToolRouter;
    use std::sync::Arc;

    fn ctx() -> Arc<ToolContext> {
        Arc::new(ToolContext::new(
            crate::tools::ServerConfig {
                kicad_cli: "kicad-cli".to_string(),
                kicad_binary: "kicad".to_string(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        ))
    }

    fn body(result: &CallToolResult) -> Value {
        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text content")
        };
        serde_json::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn a_task_survives_the_call_that_created_it() {
        let ctx = ctx();
        let started = body(
            &handle_start_task(
                &json!({
                    "objective": "Add the 3V3 rail for the STM32",
                    "constraints": ["do not touch the RF section"],
                    "success_criteria": ["ERC = 0"]
                }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        let id = started["task_id"].as_str().unwrap().to_string();
        assert!(started["anchor"]
            .as_str()
            .unwrap()
            .contains("do not touch the RF section"));

        let fetched = body(
            &handle_get_task(&json!({"task_id": id}), &ctx)
                .await
                .unwrap(),
        );
        assert_eq!(fetched["task"]["success_criteria"][0], "ERC = 0");
        assert_eq!(fetched["task"]["status"], "active");
    }

    #[tokio::test]
    async fn an_update_appends_and_returns_only_the_anchor() {
        let ctx = ctx();
        let started = body(
            &handle_start_task(&json!({"objective": "wire the LDO"}), &ctx)
                .await
                .unwrap(),
        );
        let id = started["task_id"].as_str().unwrap().to_string();

        let updated = body(
            &handle_update_task(
                &json!({
                    "task_id": id,
                    "subgoal": "output caps",
                    "progress": {"done": 2, "total": 4},
                    "verified_facts": ["ERC is 0 at revision 4f2a"],
                    "failed": {"what": "place C2 at 100,80", "why": "collides with U1"}
                }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert!(updated["task"].is_null(), "an update is not a dump");
        assert!(updated["anchor"].as_str().unwrap().contains("2/4"));

        let task = ctx.tasks.get(&id).unwrap();
        assert_eq!(task.verified_facts.len(), 1);
        assert_eq!(task.failed_attempts[0].why, "collides with U1");
    }

    #[tokio::test]
    async fn a_refused_constraint_is_reported_not_swallowed() {
        let ctx = ctx();
        let started = body(
            &handle_start_task(&json!({"objective": "o"}), &ctx)
                .await
                .unwrap(),
        );
        let id = started["task_id"].as_str().unwrap();
        let many: Vec<String> = (0..(kam_state::task::MAX_LIST + 1))
            .map(|i| format!("c{i}"))
            .collect();

        let updated = body(
            &handle_update_task(&json!({"task_id": id, "constraints": many}), &ctx)
                .await
                .unwrap(),
        );
        let refused = updated["refused"].as_array().expect("must be reported");
        assert_eq!(refused.len(), 1);
        assert!(refused[0].as_str().unwrap().contains("task_list_full"));
    }

    #[tokio::test]
    async fn an_unknown_task_is_a_structured_error() {
        let ctx = ctx();
        let result = handle_update_task(&json!({"task_id": "task_99"}), &ctx)
            .await
            .unwrap();
        assert!(result.is_error);
        assert_eq!(body(&result)["error"]["kind"], "invalid_argument");
    }

    #[tokio::test]
    async fn an_unknown_status_is_refused_rather_than_ignored() {
        let ctx = ctx();
        let started = body(
            &handle_start_task(&json!({"objective": "o"}), &ctx)
                .await
                .unwrap(),
        );
        let id = started["task_id"].as_str().unwrap();
        let result = handle_update_task(&json!({"task_id": id, "status": "finished"}), &ctx)
            .await
            .unwrap();
        assert!(result.is_error);
        assert_eq!(body(&result)["error"]["field"], "status");
        // And the task is untouched, rather than half-updated.
        assert_eq!(ctx.tasks.get(id).unwrap().status, TaskStatus::Active);
    }
}
