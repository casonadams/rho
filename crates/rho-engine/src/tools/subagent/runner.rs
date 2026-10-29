use crate::engine::compactor::llm::ModelHandle;
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use crate::tools::engine_tool::EngineTool;
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use rho_harness_core::model::{
    AssistantContent, ChatMessage, TextContent, ToolCall, ToolResult as DomainToolResult, UserContent,
};
use std::sync::Arc;
use std::time::Duration;

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
    role: SubagentRole,
    custom_preamble: Option<String>,
    tools: Vec<Arc<dyn EngineTool>>,
    max_turns: usize,
}

fn create_assistant_tool_message(content: &str, calls: &[ToolCall]) -> ChatMessage {
    let mut asst_content = Vec::new();
    if !content.is_empty() {
        asst_content.push(AssistantContent::Text(TextContent::new(content)));
    }
    for call in calls {
        asst_content.push(AssistantContent::ToolCall(call.clone()));
    }
    ChatMessage::Assistant {
        id: None,
        content: asst_content,
    }
}

async fn dispatch_subagent_tool_calls(calls: &[ToolCall], tools: &[Arc<dyn EngineTool>]) -> Vec<ChatMessage> {
    let mut result_messages = Vec::new();
    for call in calls {
        let tool = tools.iter().find(|t| t.name() == call.function.name);
        let (output, _is_error) = match tool {
            Some(t) => match t.execute(call.function.arguments.clone()).await {
                Ok(res) => (res.content, res.is_error),
                Err(e) => (format!("Error: {e}"), true),
            },
            None => (format!("Error: Unknown tool '{}'", call.function.name), true),
        };
        result_messages.push(ChatMessage::User {
            content: vec![UserContent::ToolResult(DomainToolResult::new(
                call.id.clone(),
                call.function.name.clone(),
                output,
            ))],
        });
    }
    result_messages
}

impl SubagentRunner {
    #[must_use]
    pub fn new(
        model: ModelHandle,
        role: SubagentRole,
        available_tools: &[Arc<dyn EngineTool>],
        max_turns: usize,
    ) -> Self {
        let tools = filter_tools_for_role(available_tools, role);
        Self {
            model,
            role,
            custom_preamble: None,
            tools,
            max_turns,
        }
    }

    #[must_use]
    pub fn new_custom(model: ModelHandle, preamble: String, tools: Vec<Arc<dyn EngineTool>>, max_turns: usize) -> Self {
        Self {
            model,
            role: SubagentRole::General,
            custom_preamble: Some(preamble),
            tools,
            max_turns,
        }
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub async fn run(&self, prompt: &str, context_slice: Option<&str>) -> ToolResult {
        if prompt.trim().is_empty() {
            return ToolResult::error("subagent prompt cannot be empty");
        }

        let full_prompt = build_subagent_prompt(prompt, context_slice);
        let instructions = self
            .custom_preamble
            .as_deref()
            .unwrap_or_else(|| self.role.system_instructions());
        let preamble = format!(
            "{instructions}\n\nYou have a budget of {} turns. Use early turns for targeted tool calls, and reserve remaining turns to synthesize your final findings.",
            self.max_turns
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

        let mut messages: Vec<ChatMessage> = Vec::new();

        for turn in 1..=self.max_turns {
            let req = ModelCompletionRequest {
                preamble: Some(preamble.clone()),
                prompt: if turn == 1 { full_prompt.clone() } else { String::new() },
                messages: messages.clone(),
                tools: tools_def.clone(),
                max_tokens: None,
                temperature: Some(0.0),
            };

            let resp = match self.model.complete(req).await {
                Ok(r) => r,
                Err(e) => return ToolResult::error(format!("Subagent model call failed: {e}")),
            };

            if resp.tool_calls.is_empty() {
                return handle_runner_success(&resp.content);
            }

            let asst = create_assistant_tool_message(&resp.content, &resp.tool_calls);
            messages.push(asst);
            let tool_results = dispatch_subagent_tool_calls(&resp.tool_calls, &self.tools).await;
            messages.extend(tool_results);
        }

        if let Some(synthesis) = attempt_recovery_synthesis(&self.model, self.role, &messages).await {
            return ToolResult::success(synthesis);
        }
        synthesize_budget_exhausted_result(self.max_turns, &messages)
    }
}

fn handle_runner_success(output: &str) -> ToolResult {
    let text = output.trim();
    if text.is_empty() {
        ToolResult::error("Subagent completed without generating an output summary.")
    } else {
        ToolResult::success(text)
    }
}

pub(crate) fn prepare_history_for_recovery(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut end = history.len();
    while end > 0 {
        match &history[end - 1] {
            ChatMessage::Assistant { content, .. }
                if content.iter().any(|c| matches!(c, AssistantContent::ToolCall(_))) =>
            {
                end -= 1
            }
            _ => break,
        }
    }
    history[..end].to_vec()
}

async fn attempt_recovery_synthesis(
    model: &ModelHandle,
    role: SubagentRole,
    history: &[ChatMessage],
) -> Option<String> {
    let sanitized_history = prepare_history_for_recovery(history);
    let preamble = format!(
        "{}\n\nYour tool exploration is complete. Synthesize a structured summary of your findings addressing the user's task. Do not call tools.",
        role.system_instructions()
    );
    let prompt = "Please synthesize your final structured findings and summary based on the findings above.";
    let req = ModelCompletionRequest {
        preamble: Some(preamble),
        prompt: prompt.to_string(),
        messages: sanitized_history,
        tools: Vec::new(),
        max_tokens: None,
        temperature: Some(0.0),
    };
    let resp = tokio::time::timeout(Duration::from_secs(45), model.complete(req))
        .await
        .ok()?
        .ok()?;
    let text = resp.content.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn extract_assistant_text(asst: &AssistantContent, out: &mut Vec<String>) {
    if let AssistantContent::Text(t) = asst {
        let trimmed = t.text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
}

fn extract_tool_result_text(res: &DomainToolResult, out: &mut Vec<String>) {
    for item in &res.content {
        if let rho_harness_core::model::ToolResultContent::Text(t) = item {
            let trimmed = t.text.trim();
            if !trimmed.is_empty() {
                let snippet: String = trimmed.chars().take(200).collect();
                out.push(format!("Tool {}: {snippet}", res.name));
            }
        }
    }
}

fn collect_history_findings(history: &[ChatMessage]) -> (Vec<String>, Vec<String>) {
    let mut texts = Vec::new();
    let mut tools = Vec::new();
    for msg in history {
        match msg {
            ChatMessage::Assistant { content, .. } => {
                for c in content {
                    extract_assistant_text(c, &mut texts);
                }
            }
            ChatMessage::User { content } => {
                for c in content {
                    if let UserContent::ToolResult(res) = c {
                        extract_tool_result_text(res, &mut tools);
                    }
                }
            }
            _ => {}
        }
    }
    (texts, tools)
}

pub(crate) fn synthesize_budget_exhausted_result(max_turns: usize, history: &[ChatMessage]) -> ToolResult {
    let (texts, tools) = collect_history_findings(history);
    if !texts.is_empty() {
        let findings = texts.join("\n\n");
        ToolResult::success(format!(
            "Subagent reached turn limit of {max_turns}. Gathered findings:\n\n{findings}"
        ))
    } else if !tools.is_empty() {
        let findings = tools.join("\n");
        ToolResult::success(format!(
            "Subagent reached turn limit of {max_turns} during exploration. Discovered:\n\n{findings}"
        ))
    } else {
        ToolResult::error(format!(
            "Subagent reached turn limit of {max_turns} without producing output"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::MockAdapter;

    #[test]
    fn test_prompt_formatting() {
        assert_eq!(build_subagent_prompt("do work", None), "do work");
        assert_eq!(build_subagent_prompt("do work", Some("")), "do work");
        assert_eq!(
            build_subagent_prompt("do work", Some("important background")),
            "Context:\nimportant background\n\nTask:\ndo work"
        );
    }

    #[tokio::test]
    async fn test_subagent_runner_empty_prompt() {
        let model = Arc::new(MockAdapter::text("done"));
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("   ", None).await;
        assert!(res.is_error);
        assert!(res.content.contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_subagent_runner_success_flow() {
        let model = Arc::new(MockAdapter::text("Scout findings summary"));
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("investigate directory", None).await;
        assert!(!res.is_error);
        assert_eq!(res.content, "Scout findings summary");
    }
}
