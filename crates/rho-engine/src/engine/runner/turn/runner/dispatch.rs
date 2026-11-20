use crate::engine::runner::sink::TerminalApprovalSink;
use crate::hook::{LifecycleHook, ToolDispatchAction};
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use rho_harness_core::model::{ChatMessage, ToolCall};
use std::sync::Arc;

pub(super) struct ToolExecutionContext<'a> {
    pub(super) tools: &'a [Arc<dyn EngineTool>],
    pub(super) sink: &'a Arc<TerminalApprovalSink>,
    pub(super) repeat_tracker: &'a mut RepeatedCallTracker,
    pub(super) lifecycle: Option<&'a Arc<LifecycleHook>>,
    pub(super) permission: Option<&'a Arc<crate::permission::PermissionHook>>,
}

pub(crate) enum DispatchDecision {
    Proceed(serde_json::Value),
    Deny(ChatMessage),
    Stop(ChatMessage, String),
}

pub(crate) async fn evaluate_tool_dispatch(
    call: &ToolCall,
    lifecycle: Option<&Arc<LifecycleHook>>,
    permission: Option<&Arc<crate::permission::PermissionHook>>,
) -> DispatchDecision {
    let mut args = crate::tools::types::normalize_tool_arguments(call.function.arguments.clone());
    if let Some(lifecycle) = lifecycle {
        match lifecycle.on_tool_dispatch(&call.function.name, &args).await {
            ToolDispatchAction::Stop(reason) => {
                let msg = tool_result_message(call, format!("Stopped by hook: {reason}"));
                return DispatchDecision::Stop(msg, reason);
            }
            ToolDispatchAction::Deny(reason) => {
                let msg = tool_result_message(call, format!("Denied by hook: {reason}"));
                return DispatchDecision::Deny(msg);
            }
            ToolDispatchAction::RewriteArgs(new_args) => args = new_args,
            ToolDispatchAction::Proceed => {}
        }
    }

    if let Some(permission) = permission {
        match permission.check_permission(&call.function.name, &args).await {
            crate::permission::hook::PermissionAction::Deny(reason) => {
                let msg = tool_result_message(call, format!("Permission denied: {reason}"));
                return DispatchDecision::Deny(msg);
            }
            crate::permission::hook::PermissionAction::RewriteArgs(new_args) => args = new_args,
            crate::permission::hook::PermissionAction::Proceed => {}
        }
    }

    DispatchDecision::Proceed(args)
}

pub(crate) async fn apply_tool_outcome_hooks(
    lifecycle: Option<&Arc<LifecycleHook>>,
    tool_name: &str,
    args: &serde_json::Value,
    output: &mut String,
    is_error: bool,
) -> Option<String> {
    let lifecycle = lifecycle?;
    match lifecycle.on_tool_outcome(tool_name, args, output, is_error).await {
        crate::hook::ToolOutcomeAction::Proceed => None,
        crate::hook::ToolOutcomeAction::RewriteResult(new_output) => {
            *output = new_output;
            None
        }
        crate::hook::ToolOutcomeAction::Stop(reason) => Some(reason),
    }
}

type CallOutcome = (ChatMessage, Option<String>);

fn tool_result_message(call: &ToolCall, output: String) -> ChatMessage {
    ChatMessage::User {
        content: vec![rho_harness_core::model::UserContent::ToolResult(
            rho_harness_core::model::ToolResult::new(call.id.clone(), call.function.name.clone(), output),
        )],
    }
}

enum PlannedCall {
    Ready(serde_json::Value),
    Settled(CallOutcome),
}

async fn plan_tool_call(call: &ToolCall, ctx: &mut ToolExecutionContext<'_>) -> PlannedCall {
    if let Err(msg) = ctx.repeat_tracker.check(&call.function.name, &call.function.arguments) {
        return PlannedCall::Settled((tool_result_message(call, msg.to_string()), None));
    }
    match evaluate_tool_dispatch(call, ctx.lifecycle, ctx.permission).await {
        DispatchDecision::Proceed(args) => {
            ctx.sink.tool_start(&call.function.name, &args);
            PlannedCall::Ready(args)
        }
        DispatchDecision::Deny(msg) => PlannedCall::Settled((msg, None)),
        DispatchDecision::Stop(msg, reason) => PlannedCall::Settled((msg, Some(reason))),
    }
}

struct ToolRun {
    output: String,
    display: Option<String>,
    is_error: bool,
}

impl ToolRun {
    fn failed(output: String) -> Self {
        Self {
            output,
            display: None,
            is_error: true,
        }
    }
}

async fn run_planned_tool(tools: &[Arc<dyn EngineTool>], call: &ToolCall, args: &serde_json::Value) -> ToolRun {
    match tools.iter().find(|t| t.name() == call.function.name) {
        Some(t) => match t.execute(args.clone()).await {
            Ok(r) => ToolRun {
                output: r.content,
                display: r.display,
                is_error: r.is_error,
            },
            Err(e) => ToolRun::failed(format!("Error: {e}")),
        },
        None => ToolRun::failed(format!("Error: Tool '{}' not found", call.function.name)),
    }
}

async fn finish_tool_call(
    call: &ToolCall,
    args: &serde_json::Value,
    run: ToolRun,
    ctx: &ToolExecutionContext<'_>,
) -> CallOutcome {
    let ToolRun {
        mut output,
        display,
        is_error,
    } = run;
    ctx.sink.tool_finished(crate::engine::runner::sink::ToolFinishDetails {
        name: &call.function.name,
        arguments: args,
        output: display.as_deref().unwrap_or(&output),
        is_error,
    });
    let stop_reason = apply_tool_outcome_hooks(ctx.lifecycle, &call.function.name, args, &mut output, is_error).await;
    (tool_result_message(call, output), stop_reason)
}

pub(super) async fn execute_single_tool_call(call: &ToolCall, ctx: &mut ToolExecutionContext<'_>) -> CallOutcome {
    match plan_tool_call(call, ctx).await {
        PlannedCall::Settled(outcome) => outcome,
        PlannedCall::Ready(args) => {
            let result = run_planned_tool(ctx.tools, call, &args).await;
            finish_tool_call(call, &args, result, ctx).await
        }
    }
}

/// Plans every call in order (so permission prompts never overlap), runs the approved ones together, then finishes them in order.
pub(super) async fn execute_concurrent_tool_calls(
    calls: &[ToolCall],
    ctx: &mut ToolExecutionContext<'_>,
) -> Vec<CallOutcome> {
    let mut planned = Vec::with_capacity(calls.len());
    for call in calls {
        let plan = plan_tool_call(call, ctx).await;
        let stops = matches!(&plan, PlannedCall::Settled((_, Some(_))));
        planned.push(plan);
        if stops {
            break;
        }
    }
    let tools = ctx.tools;
    let results = futures::future::join_all(planned.iter().zip(calls).map(|(plan, call)| async move {
        match plan {
            PlannedCall::Ready(args) => Some(run_planned_tool(tools, call, args).await),
            PlannedCall::Settled(_) => None,
        }
    }))
    .await;

    let mut outcomes = Vec::with_capacity(planned.len());
    for ((plan, result), call) in planned.into_iter().zip(results).zip(calls) {
        match (plan, result) {
            (PlannedCall::Ready(args), Some(result)) => outcomes.push(finish_tool_call(call, &args, result, ctx).await),
            (PlannedCall::Settled(outcome), _) => outcomes.push(outcome),
            (PlannedCall::Ready(_), None) => {}
        }
    }
    outcomes
}

pub(super) fn batch_is_concurrency_safe(tools: &[Arc<dyn EngineTool>], calls: &[ToolCall]) -> bool {
    calls.len() > 1
        && calls.iter().all(|call| {
            tools
                .iter()
                .find(|t| t.name() == call.function.name)
                .is_some_and(|t| t.concurrency_safe(&call.function.arguments))
        })
}
