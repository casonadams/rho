use super::recovery::{attempt_recovery_synthesis, partial_result};
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::StructuralUsage;
use crate::engine::runner::turn::runner::{DispatchDecision, apply_tool_outcome_hooks, evaluate_tool_dispatch};
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use rho_harness_core::model::{AssistantContent, ChatMessage, TextContent, ToolCall, UserContent};
use serde_json::Value;
use std::sync::Arc;

const MAX_TOOL_OUTPUT_CHARS: usize = 20_000;
const MAX_PARENT_MESSAGES: usize = 20;
const MAX_PARENT_MESSAGE_CHARS: usize = 1_500;
pub(crate) const READ_ONLY_TOOLS: [&str; 5] = ["read", "fd", "rg", "web_fetch", "web_search"];

pub fn build_subagent_prompt(prompt: &str, context_slice: Option<&str>) -> String {
    match context_slice {
        Some(slice) if !slice.trim().is_empty() => {
            format!("Context:\n{}\n\nTask:\n{}", slice.trim(), prompt.trim())
        }
        _ => prompt.trim().to_string(),
    }
}

pub fn filter_tools_for_role(tools: &[Arc<dyn EngineTool>], role: SubagentRole) -> Vec<Arc<dyn EngineTool>> {
    let allowed = role.allowed_tools();
    let mut filtered: Vec<Arc<dyn EngineTool>> =
        tools.iter().filter(|t| allowed.contains(&t.name())).cloned().collect();
    filtered.sort_by(|a, b| a.name().cmp(b.name()));
    filtered
}

pub struct SubagentRunner {
    model: ModelHandle,
    instructions: String,
    tools: Vec<Arc<dyn EngineTool>>,
    max_turns: usize,
    ctx: TurnToolContext,
    label: String,
}

struct RunState {
    task: String,
    messages: Vec<ChatMessage>,
    usage: StructuralUsage,
    turns: usize,
    tool_calls: usize,
}

fn tool_result_message(call: &ToolCall, output: String) -> ChatMessage {
    ChatMessage::User {
        content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
            call.id.clone(),
            call.function.name.clone(),
            output,
        ))],
    }
}

fn assistant_tool_message(content: &str, calls: &[ToolCall]) -> ChatMessage {
    let mut parts = Vec::new();
    if !content.is_empty() {
        parts.push(AssistantContent::Text(TextContent::new(content)));
    }
    parts.extend(calls.iter().cloned().map(AssistantContent::ToolCall));
    ChatMessage::Assistant {
        id: None,
        content: parts,
    }
}

fn truncate_output(output: String) -> String {
    let total = output.chars().count();
    if total <= MAX_TOOL_OUTPUT_CHARS {
        return output;
    }
    let kept: String = output.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    format!("{kept}\n[truncated {} characters]", total - MAX_TOOL_OUTPUT_CHARS)
}

fn message_text(message: &ChatMessage) -> Option<(&'static str, String)> {
    let (speaker, text) = match message {
        ChatMessage::User { content } => (
            "User",
            content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::Assistant { content, .. } => (
            "Assistant",
            content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::System { .. } => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| (speaker, text.chars().take(MAX_PARENT_MESSAGE_CHARS).collect()))
}

/// Text-only transcript of the most recent parent messages; tool calls, tool output and reasoning are left out.
pub(crate) fn render_parent_conversation(history: &[ChatMessage]) -> String {
    let lines: Vec<String> = history
        .iter()
        .filter_map(message_text)
        .map(|(speaker, text)| format!("{speaker}: {text}"))
        .collect();
    let recent = &lines[lines.len().saturating_sub(MAX_PARENT_MESSAGES)..];
    if recent.is_empty() {
        return String::new();
    }
    format!("Parent conversation (most recent last):\n{}", recent.join("\n\n"))
}

fn interrupted() -> ToolResult {
    ToolResult::error("Subagent interrupted by operator")
}

impl SubagentRunner {
    #[must_use]
    pub fn new(
        model: ModelHandle,
        role: SubagentRole,
        available_tools: &[Arc<dyn EngineTool>],
        max_turns: usize,
    ) -> Self {
        Self::new_custom(
            model,
            role.system_instructions().to_string(),
            filter_tools_for_role(available_tools, role),
            max_turns,
        )
    }

    #[must_use]
    pub fn new_custom(
        model: ModelHandle,
        instructions: String,
        tools: Vec<Arc<dyn EngineTool>>,
        max_turns: usize,
    ) -> Self {
        Self {
            model,
            instructions,
            tools,
            max_turns,
            ctx: TurnToolContext::default(),
            label: "subagent".to_string(),
        }
    }

    #[must_use]
    pub fn with_label(mut self, name: &str) -> Self {
        self.label = format!("subagent {name}");
        self
    }

    #[must_use]
    pub fn with_context(mut self, ctx: TurnToolContext) -> Self {
        self.ctx = ctx;
        self
    }

    fn is_cancelled(&self) -> bool {
        self.ctx.cancellation.as_ref().is_some_and(|c| c.is_cancelled())
    }

    /// Resolves to `None` when the operator cancels before `fut` completes.
    async fn cancellable<T>(&self, fut: impl std::future::Future<Output = T>) -> Option<T> {
        let Some(cancel) = &self.ctx.cancellation else {
            return Some(fut.await);
        };
        tokio::select! {
            value = fut => Some(value),
            () = cancel.wait_cancelled() => None,
        }
    }

    pub async fn run(&self, prompt: &str, context_slice: Option<&str>) -> ToolResult {
        if prompt.trim().is_empty() {
            return ToolResult::error("subagent prompt cannot be empty");
        }

        let preamble = format!(
            "{}\n\nYou have a budget of {} turns. Use early turns for targeted tool calls, and reserve remaining turns to synthesize your final findings.\n\nEnd with a final answer containing: a one-paragraph summary, findings with file:line references, files changed (if any), and open questions.",
            self.instructions, self.max_turns
        );
        let tools_def: Vec<ModelToolDefinition> = self
            .tools
            .iter()
            .map(|t| ModelToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect();
        let mut state = RunState {
            task: build_subagent_prompt(prompt, context_slice),
            messages: Vec::new(),
            usage: StructuralUsage::default(),
            turns: 0,
            tool_calls: 0,
        };

        for turn in 1..=self.max_turns {
            if let Some(result) = self.step(turn, &preamble, &tools_def, &mut state).await {
                return result;
            }
        }
        self.finish_exhausted(&state).await
    }

    /// Runs one model turn; `Some` ends the run.
    async fn step(
        &self,
        turn: usize,
        preamble: &str,
        tools_def: &[ModelToolDefinition],
        state: &mut RunState,
    ) -> Option<ToolResult> {
        if self.is_cancelled() {
            return Some(interrupted());
        }
        let req = ModelCompletionRequest {
            preamble: Some(preamble.to_string()),
            prompt: if turn == 1 { state.task.clone() } else { String::new() },
            messages: state.messages.clone(),
            tools: tools_def.to_vec(),
            max_tokens: None,
            temperature: Some(0.0),
        };
        let resp = match self.cancellable(self.model.complete(req)).await {
            None => return Some(interrupted()),
            Some(Err(e)) => {
                let reason = format!("stopped after a model call failed ({e})");
                return Some(partial_result(&reason, &state.messages));
            }
            Some(Ok(resp)) => resp,
        };
        state.usage = state.usage.merge(&resp.usage);
        state.turns = turn;

        if resp.tool_calls.is_empty() {
            return Some(Self::finish(&resp.content, state));
        }
        self.report_progress(turn, &resp.tool_calls);
        if state.messages.is_empty() {
            state.messages.push(ChatMessage::user(state.task.clone()));
        }
        state
            .messages
            .push(assistant_tool_message(&resp.content, &resp.tool_calls));
        self.dispatch(&resp.tool_calls, state).await.err()
    }

    fn report_progress(&self, turn: usize, calls: &[ToolCall]) {
        let names: Vec<&str> = calls.iter().map(|c| c.function.name.as_str()).collect();
        self.ctx.progress.stream_chunk(&format!(
            "{} turn {turn}/{}: {}\n",
            self.label,
            self.max_turns,
            names.join(", ")
        ));
    }

    fn finish(text: &str, state: &RunState) -> ToolResult {
        let text = text.trim();
        if text.is_empty() {
            return ToolResult::error("Subagent completed without generating an output summary.");
        }
        let tokens = state.usage.input_tokens + state.usage.output_tokens;
        ToolResult::success(format!(
            "{text}\n\n[subagent: {} turns, {} tool calls, {tokens} tokens]",
            state.turns, state.tool_calls
        ))
    }

    async fn finish_exhausted(&self, state: &RunState) -> ToolResult {
        if self.is_cancelled() {
            return interrupted();
        }
        let synthesis = self
            .cancellable(attempt_recovery_synthesis(
                &self.model,
                &self.instructions,
                &state.messages,
            ))
            .await;
        match synthesis {
            None => interrupted(),
            Some(Some(text)) => Self::finish(&text, state),
            Some(None) => partial_result(&format!("reached turn limit of {}", self.max_turns), &state.messages),
        }
    }

    /// Applies hooks and permissions to every call in order, then executes the approved ones.
    async fn dispatch(&self, calls: &[ToolCall], state: &mut RunState) -> Result<(), ToolResult> {
        let mut slots: Vec<Option<ChatMessage>> = vec![None; calls.len()];
        let mut approved: Vec<(usize, &ToolCall, Value)> = Vec::new();
        for (index, call) in calls.iter().enumerate() {
            let decision = self
                .cancellable(evaluate_tool_dispatch(
                    call,
                    self.ctx.lifecycle.as_ref(),
                    self.ctx.permission.as_ref(),
                ))
                .await
                .ok_or_else(interrupted)?;
            match decision {
                DispatchDecision::Proceed(args) => approved.push((index, call, args)),
                DispatchDecision::Deny(msg) => slots[index] = Some(msg),
                DispatchDecision::Stop(_, reason) => {
                    return Err(ToolResult::error(format!("Subagent stopped by hook: {reason}")));
                }
            }
        }

        let concurrent = approved.len() > 1
            && approved
                .iter()
                .all(|(_, call, _)| READ_ONLY_TOOLS.contains(&call.function.name.as_str()));
        let outcomes = if concurrent {
            futures::future::join_all(approved.iter().map(|(_, call, args)| self.run_call(call, args))).await
        } else {
            let mut sequential = Vec::with_capacity(approved.len());
            for (_, call, args) in &approved {
                sequential.push(self.run_call(call, args).await);
            }
            sequential
        };

        let mut stop = None;
        for ((index, _, _), (msg, stop_reason)) in approved.iter().zip(outcomes) {
            slots[*index] = Some(msg);
            stop = stop.or(stop_reason);
        }
        state.tool_calls += calls.len();
        state.messages.extend(slots.into_iter().flatten());
        match stop {
            Some(reason) => Err(ToolResult::error(format!("Subagent stopped by hook: {reason}"))),
            None => Ok(()),
        }
    }

    async fn run_call(&self, call: &ToolCall, args: &Value) -> (ChatMessage, Option<String>) {
        let name = call.function.name.as_str();
        let (mut output, is_error) = self
            .cancellable(self.run_tool(name, args))
            .await
            .unwrap_or_else(|| ("Cancelled by operator".to_string(), true));
        let stop = apply_tool_outcome_hooks(self.ctx.lifecycle.as_ref(), name, args, &mut output, is_error).await;
        (tool_result_message(call, truncate_output(output)), stop)
    }

    async fn run_tool(&self, name: &str, args: &Value) -> (String, bool) {
        let Some(tool) = self.tools.iter().find(|t| t.name() == name) else {
            return (format!("Error: Unknown tool '{name}'"), true);
        };
        match tool.execute(args.clone()).await {
            Ok(res) => (res.content, res.is_error),
            Err(e) => (format!("Error: {e}"), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_formatting() {
        assert_eq!(build_subagent_prompt("do work", None), "do work");
        assert_eq!(build_subagent_prompt("do work", Some("")), "do work");
        assert_eq!(
            build_subagent_prompt("do work", Some("important background")),
            "Context:\nimportant background\n\nTask:\ndo work"
        );
    }

    #[test]
    fn truncate_output_caps_length_on_char_boundaries() {
        let short = "héllo".to_string();
        assert_eq!(truncate_output(short.clone()), short);

        let long = "é".repeat(MAX_TOOL_OUTPUT_CHARS + 5);
        let cut = truncate_output(long);
        assert!(cut.ends_with("\n[truncated 5 characters]"));
        assert_eq!(cut.chars().filter(|c| *c == 'é').count(), MAX_TOOL_OUTPUT_CHARS);
    }
}
