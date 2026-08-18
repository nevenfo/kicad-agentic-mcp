//! End-to-end explicit Agent execution: proposal, Plan IR, apply, verification.
//!
//! Model text is an untrusted proposal until existing Plan IR compilation,
//! execution and deterministic `kicad-cli` verification complete.

use crate::mcp::protocol::CallToolResult;
use crate::tools::ToolContext;
use kam_runtime::{RoutingDecision, SupervisorInput};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Structured end-to-end Agent result.
#[derive(Debug, Clone, Serialize)]
pub struct AgentLoopOutcome {
    /// `SUCCESS`, `MODEL_FAILED`, `PLAN_INVALID`, `PREVIEW_FAILED`,
    /// `APPLY_FAILED`, or `VERIFICATION_FAILED`.
    pub status: &'static str,
    /// Local model name/usage evidence from the supervisor turn.
    pub supervisor: kam_runtime::SupervisorOutcome,
    /// Model proposal retained as untrusted evidence.
    pub plan_ir: Option<Value>,
    /// Existing deterministic preview result.
    pub preview: Option<CallToolResult>,
    /// Existing deterministic apply result.
    pub application: Option<CallToolResult>,
    /// Existing deterministic verification result.
    pub verification: Option<crate::verification_agent::VerificationOutcome>,
    /// Stable failure explanation; success never carries one.
    pub reason: Option<String>,
}

/// Execute one LOCAL Agent turn all the way through existing Plan IR tooling.
pub async fn execute(
    ctx: Arc<ToolContext>,
    input: SupervisorInput,
    document: &str,
) -> AgentLoopOutcome {
    let supervisor = match ctx.supervisor.run(RoutingDecision::Local, input).await {
        Ok(outcome) => outcome,
        Err(error) => return failed_without_supervisor("MODEL_FAILED", error.to_string()),
    };
    let Some(proposal) = supervisor.proposal.clone() else {
        return AgentLoopOutcome {
            status: "MODEL_FAILED",
            supervisor,
            plan_ir: None,
            preview: None,
            application: None,
            verification: None,
            reason: Some("local Provider did not return a Plan IR proposal".to_string()),
        };
    };
    let plan_ir = match parse_plan_ir(&proposal) {
        Ok(plan) => plan,
        Err(reason) => {
            return AgentLoopOutcome {
                status: "PLAN_INVALID",
                supervisor,
                plan_ir: None,
                preview: None,
                application: None,
                verification: None,
                reason: Some(reason),
            }
        }
    };
    let preview = match run_plan_tool(&ctx, "preview_plan", &plan_ir).await {
        Ok(result) if !result.is_error => result,
        Ok(result) => {
            return AgentLoopOutcome {
                status: "PREVIEW_FAILED",
                supervisor,
                plan_ir: Some(plan_ir),
                preview: Some(result),
                application: None,
                verification: None,
                reason: Some("existing Plan IR preview rejected the proposal".to_string()),
            }
        }
        Err(error) => {
            return AgentLoopOutcome {
                status: "PREVIEW_FAILED",
                supervisor,
                plan_ir: Some(plan_ir),
                preview: None,
                application: None,
                verification: None,
                reason: Some(error),
            }
        }
    };
    let application = match run_plan_tool(&ctx, "apply_plan", &plan_ir).await {
        Ok(result) if !result.is_error => result,
        Ok(result) => {
            return AgentLoopOutcome {
                status: "APPLY_FAILED",
                supervisor,
                plan_ir: Some(plan_ir),
                preview: Some(preview),
                application: Some(result),
                verification: None,
                reason: Some(
                    "existing Plan IR executor rejected or rolled back the proposal".to_string(),
                ),
            }
        }
        Err(error) => {
            return AgentLoopOutcome {
                status: "APPLY_FAILED",
                supervisor,
                plan_ir: Some(plan_ir),
                preview: Some(preview),
                application: None,
                verification: None,
                reason: Some(error),
            }
        }
    };
    let verification = match ctx
        .verification_agent
        .verify(&supervisor.task.id, document)
        .await
    {
        Ok(verification) => verification,
        Err(error) => {
            return AgentLoopOutcome {
                status: "VERIFICATION_FAILED",
                supervisor,
                plan_ir: Some(plan_ir),
                preview: Some(preview),
                application: Some(application),
                verification: None,
                reason: Some(error.to_string()),
            }
        }
    };
    if verification.verdict != "PASS" {
        return AgentLoopOutcome {
            status: "VERIFICATION_FAILED",
            supervisor,
            plan_ir: Some(plan_ir),
            preview: Some(preview),
            application: Some(application),
            verification: Some(verification),
            reason: Some("deterministic validator did not return PASS".to_string()),
        };
    }
    AgentLoopOutcome {
        status: "SUCCESS",
        supervisor,
        plan_ir: Some(plan_ir),
        preview: Some(preview),
        application: Some(application),
        verification: Some(verification),
        reason: None,
    }
}

fn parse_plan_ir(proposal: &str) -> Result<Value, String> {
    let value = serde_json::from_str::<Value>(proposal)
        .map_err(|error| format!("model proposal is not JSON Plan IR: {error}"))?;
    match value {
        Value::Object(plan) if plan.contains_key("ops") => Ok(Value::Object(plan)),
        Value::Object(mut wrapper) => match wrapper.remove("plan") {
            Some(Value::Object(plan)) if plan.contains_key("ops") => Ok(Value::Object(plan)),
            _ => Err("model proposal must be a Plan IR object containing ops".to_string()),
        },
        _ => Err("model proposal must be a Plan IR object containing ops".to_string()),
    }
}

async fn run_plan_tool(
    ctx: &Arc<ToolContext>,
    name: &str,
    plan: &Value,
) -> Result<CallToolResult, String> {
    let Some(tool) = ctx.router.find_tool_def(name) else {
        return Err(format!("existing {name} tool is unavailable"));
    };
    (tool.handler)(&json!({"plan": plan}), ctx.clone())
        .await
        .map_err(|error| error.to_string())
}

fn failed_without_supervisor(status: &'static str, reason: String) -> AgentLoopOutcome {
    AgentLoopOutcome {
        status,
        supervisor: kam_runtime::SupervisorOutcome {
            task: kam_state::TaskState::new("unavailable", "unavailable"),
            decision: "LOCAL",
            local_model_called: false,
            usage: None,
            proposal: None,
            evidence: vec![],
        },
        plan_ir: None,
        preview: None,
        application: None,
        verification: None,
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kam_llm::{
        CompletionRequest, CompletionResponse, FinishReason, Message, Provider, ProviderError,
        Role, Usage,
    };
    use std::sync::Arc;

    struct PlanProvider(&'static str);
    #[async_trait]
    impl Provider for PlanProvider {
        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                message: Message::text(Role::Assistant, self.0),
                finish_reason: FinishReason::Stop,
                usage: Usage::default(),
            })
        }
    }

    #[tokio::test]
    async fn invalid_model_output_never_reaches_apply_or_success() {
        let outcome = run_with_plan("not json").await;
        assert_eq!(outcome.status, "PLAN_INVALID");
        assert!(outcome.application.is_none());
        assert!(outcome.verification.is_none());
    }

    #[test]
    fn apply_plan_wrapper_is_normalized_to_plan_ir() {
        let plan = parse_plan_ir(r#"{"plan":{"ops":[]}}"#).unwrap();
        assert_eq!(plan, serde_json::json!({"ops": []}));
    }

    #[tokio::test]
    async fn compile_failure_never_reaches_apply_or_success() {
        let outcome = run_with_plan(r#"{"ops":[{"op":"no_such_operation","with":{}}]}"#).await;
        assert_eq!(outcome.status, "PREVIEW_FAILED");
        assert!(outcome.application.is_none());
        assert!(outcome.verification.is_none());
    }

    #[tokio::test]
    async fn apply_failure_never_reaches_verification_or_success() {
        let outcome = run_with_plan(
            r#"{"ops":[{"op":"call","with":{"tool":"no_such_tool_xyz","args":{}}}]}"#,
        )
        .await;
        assert_eq!(outcome.status, "APPLY_FAILED");
        assert!(outcome.application.is_some());
        assert!(outcome.verification.is_none());
    }

    async fn run_with_plan(plan: &'static str) -> AgentLoopOutcome {
        let router = Arc::new(crate::router::ToolRouter::new());
        let ctx = Arc::new(ToolContext::new_with_observer_and_provider(
            crate::tools::ServerConfig {
                kicad_cli: "kicad-cli".to_string(),
                kicad_binary: "kicad".to_string(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            router,
            crate::observability::CallObserver::new(None),
            Some(Arc::new(PlanProvider(plan))),
        ));
        let task = ctx.tasks.start("make a plan");
        execute(
            ctx,
            SupervisorInput {
                task_id: task.id,
                task_core_tokens: 0,
                fixed_prefix_tokens: 0,
                retrieval: vec![],
            },
            "missing.kicad_sch",
        )
        .await
    }
}
