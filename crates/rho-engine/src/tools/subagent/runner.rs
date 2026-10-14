use crate::engine::compactor::llm::ModelHandle;
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use rig::tool::DynamicTool;
use std::time::Duration;

pub fn build_subagent_prompt(prompt: &str, context_slice: Option<&str>) -> String {
    match context_slice {
        Some(slice) if !slice.trim().is_empty() => {
            format!("Context:\n{}\n\nTask:\n{}", slice.trim(), prompt.trim())
        }
        _ => prompt.trim().to_string(),
    }
}

pub fn filter_tools_for_role(tools: &[DynamicTool], role: SubagentRole) -> Vec<DynamicTool> {
    let allowed = role.allowed_tools();
    let mut filtered: Vec<DynamicTool> = tools
        .iter()
        .filter(|t| allowed.contains(&t.name().as_str()))
        .cloned()
        .collect();
    filtered.sort_by(|a, b| a.name().cmp(b.name()));
    filtered
}

pub struct SubagentRunner {
    model: ModelHandle,
    role: SubagentRole,
    tools: Vec<DynamicTool>,
    max_turns: usize,
}

impl SubagentRunner {
    #[must_use]
    pub fn new(model: ModelHandle, role: SubagentRole, available_tools: &[DynamicTool], max_turns: usize) -> Self {
        let tools = filter_tools_for_role(available_tools, role);
        Self {
            model,
            role,
            tools,
            max_turns,
        }
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name().as_str()).collect()
    }

    async fn handle_runner_error(&self, err: rig::completion::PromptError) -> ToolResult {
        let Some((max_turns, history)) = crate::engine::runner::budget_history(&err) else {
            return ToolResult::error(format!("Subagent execution failed: {err}"));
        };
        if let Some(synthesis) = attempt_recovery_synthesis(&self.model, self.role, &history).await {
            return ToolResult::success(synthesis);
        }
        synthesize_budget_exhausted_result(max_turns, &history)
    }

    pub async fn run(&self, prompt: &str, context_slice: Option<&str>) -> ToolResult {
        if prompt.trim().is_empty() {
            return ToolResult::error("subagent prompt cannot be empty");
        }

        let full_prompt = build_subagent_prompt(prompt, context_slice);
        let tool_server = rig::tool::server::ToolServer::new().dynamic_tools(self.tools.clone());
        let tool_handle = tool_server.run();

        let preamble = format!(
            "{}\n\nYou have a budget of {} turns. Use early turns for targeted tool calls, and reserve remaining turns to synthesize your final findings.",
            self.role.system_instructions(),
            self.max_turns
        );
        let agent = rig::agent::AgentBuilder::new(self.model.clone())
            .preamble(preamble)
            .default_max_turns(self.max_turns)
            .record_content_telemetry(false)
            .tool_server_handle(tool_handle)
            .build();

        let runner = crate::engine::runtime::build_runner(&agent, full_prompt).max_turns(self.max_turns);
        let timeout = Duration::from_secs(120);

        match tokio::time::timeout(timeout, runner.run()).await {
            Ok(Ok(response)) => handle_runner_success(&response.output()),
            Ok(Err(err)) => self.handle_runner_error(err).await,
            Err(_) => ToolResult::error("Subagent execution timed out after 120s."),
        }
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

fn has_tool_calls(content: &[rig::message::AssistantContent]) -> bool {
    content
        .iter()
        .any(|c| matches!(c, rig::message::AssistantContent::ToolCall(_)))
}

pub(crate) fn prepare_history_for_recovery(history: &[rig::message::Message]) -> Vec<rig::message::Message> {
    let mut end = history.len();
    while end > 0 {
        match &history[end - 1] {
            rig::message::Message::Assistant(asst) if has_tool_calls(&asst.content) => end -= 1,
            _ => break,
        }
    }
    history[..end].to_vec()
}

async fn attempt_recovery_synthesis(
    model: &ModelHandle,
    role: SubagentRole,
    history: &[rig::message::Message],
) -> Option<String> {
    let sanitized_history = prepare_history_for_recovery(history);
    let preamble = format!(
        "{}\n\nYour tool exploration is complete. Synthesize a structured summary of your findings addressing the user's task. Do not call tools.",
        role.system_instructions()
    );
    let agent = rig::agent::AgentBuilder::new(model.clone())
        .preamble(preamble)
        .default_max_turns(1)
        .record_content_telemetry(false)
        .build();
    let prompt = "Please synthesize your final structured findings and summary based on the findings above.";
    let runner = crate::engine::runtime::build_runner(&agent, prompt)
        .history(sanitized_history)
        .max_turns(1);
    let resp = tokio::time::timeout(Duration::from_secs(45), runner.run())
        .await
        .ok()?
        .ok()?;
    let text = resp.output().trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn extract_assistant_text(asst: &rig::message::AssistantMessage, out: &mut Vec<String>) {
    for content in &asst.content {
        if let rig::message::AssistantContent::Text(t) = content {
            let trimmed = t.text.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }
}

fn single_tool_result_text(tr: &rig::message::ToolResult) -> Option<String> {
    let mut parts = Vec::new();
    for item in &tr.content {
        if let rig::message::ToolResultContent::Text(t) = item {
            parts.push(t.text.as_str());
        }
    }
    let joined = parts.join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return None;
    }
    let snippet = match trimmed.char_indices().nth(300) {
        Some((idx, _)) => format!("{}...", &trimmed[..idx]),
        None => trimmed.to_string(),
    };
    Some(format!("{}: {snippet}", tr.name))
}

fn extract_tool_result_preview(user_content: &[rig::message::UserContent], out: &mut Vec<String>) {
    for c in user_content {
        if let rig::message::UserContent::ToolResult(tr) = c
            && let Some(summary) = single_tool_result_text(tr)
        {
            out.push(summary);
        }
    }
}

fn synthesize_budget_exhausted_result(max_turns: usize, history: &[rig::message::Message]) -> ToolResult {
    let mut texts = Vec::new();
    let mut tools = Vec::new();

    for msg in history {
        match msg {
            rig::message::Message::Assistant(asst) => extract_assistant_text(asst, &mut texts),
            rig::message::Message::User { content } => extract_tool_result_preview(content, &mut tools),
            _ => {}
        }
    }

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
    use crate::adapter::rig::model::into_dyn_model;
    use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
    use rho_harness_core::error::AppError;

    struct EchoMockAdapter {
        reply: String,
    }

    #[async_trait::async_trait]
    impl ModelAdapter for EchoMockAdapter {
        fn model_name(&self) -> &str {
            "mock-subagent"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn complete(&self, _request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
            Ok(ModelCompletionResponse {
                content: self.reply.clone(),
                tool_calls: Vec::new(),
                usage: Default::default(),
            })
        }

        async fn stream(
            &self,
            _request: ModelCompletionRequest,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>
        {
            let events = vec![
                Ok(ModelStreamEvent::Text(self.reply.clone())),
                Ok(ModelStreamEvent::Usage(
                    crate::engine::metrics::StructuralUsage::default(),
                )),
            ];
            Ok(Box::pin(futures::stream::iter(events)))
        }
    }

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
        let model = into_dyn_model(EchoMockAdapter { reply: "done".into() });
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("   ", None).await;
        assert!(res.is_error);
        assert!(res.content.contains("subagent prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_subagent_runner_success() {
        let model = into_dyn_model(EchoMockAdapter {
            reply: "Found 3 files matching pattern.".into(),
        });
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("find files", Some("search in src/")).await;
        assert!(!res.is_error);
        assert_eq!(res.content, "Found 3 files matching pattern.");
    }

    #[test]
    fn test_synthesize_budget_exhausted_result() {
        let text_content = vec![rig::message::AssistantContent::text(
            "Here is what was found in the codebase.",
        )];
        let asst_msg = rig::message::Message::Assistant(rig::message::AssistantMessage::new(text_content));
        let history = vec![asst_msg];

        let result = synthesize_budget_exhausted_result(8, &history);
        assert!(!result.is_error);
        assert!(result.content.contains("reached turn limit of 8"));
        assert!(result.content.contains("Here is what was found in the codebase."));

        let empty_result = synthesize_budget_exhausted_result(8, &[]);
        assert!(empty_result.is_error);
    }

    #[test]
    fn test_single_tool_result_text_safe_unicode_boundary() {
        let mut text = "a".repeat(299);
        text.push('\u{1F980}');
        text.push_str("more text");
        let name = rig::message::ToolName::new("read").unwrap();
        let tr = rig::message::ToolResult {
            call: rig::message::CallId::from_wire("call_1"),
            name,
            content: vec![rig::message::ToolResultContent::text(text)],
            is_error: false,
        };
        let preview = single_tool_result_text(&tr).expect("has preview");
        assert!(preview.starts_with("read: aaaaa"));
        assert!(preview.ends_with("..."));
    }
}
