use crate::engine::compactor::llm::ModelHandle;
use crate::provider::adapter::ModelCompletionRequest;
use crate::tools::types::ToolResult;
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolResult as DomainToolResult, UserContent};
use std::time::Duration;

const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(45);

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

pub(crate) async fn attempt_recovery_synthesis(
    model: &ModelHandle,
    instructions: &str,
    history: &[ChatMessage],
) -> Option<String> {
    let req = ModelCompletionRequest {
        preamble: Some(format!(
            "{instructions}\n\nYour tool exploration is complete. Synthesize a structured summary of your findings addressing the user's task. Do not call tools."
        )),
        prompt: "Please synthesize your final structured findings and summary based on the findings above.".to_string(),
        messages: prepare_history_for_recovery(history),
        tools: Vec::new(),
        max_tokens: None,
        temperature: Some(0.0),
    };
    let resp = tokio::time::timeout(SYNTHESIS_TIMEOUT, model.complete(req))
        .await
        .ok()?
        .ok()?;
    let text = resp.content.trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn extract_assistant_text(content: &AssistantContent, out: &mut Vec<String>) {
    if let AssistantContent::Text(t) = content {
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
            ChatMessage::System { .. } => {}
        }
    }
    (texts, tools)
}

/// Salvages whatever the subagent gathered when it cannot finish normally; `reason` completes "Subagent ...".
pub(crate) fn partial_result(reason: &str, history: &[ChatMessage]) -> ToolResult {
    let (texts, tools) = collect_history_findings(history);
    if !texts.is_empty() {
        ToolResult::success(format!(
            "Subagent {reason}. Gathered findings:\n\n{}",
            texts.join("\n\n")
        ))
    } else if !tools.is_empty() {
        ToolResult::success(format!(
            "Subagent {reason} during exploration. Discovered:\n\n{}",
            tools.join("\n")
        ))
    } else {
        ToolResult::error(format!("Subagent {reason} without producing output"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::model::{ToolCall, ToolFunction};

    #[test]
    fn collect_history_findings_splits_assistant_text_from_tool_output() {
        let long_output = "x".repeat(500);
        let history = vec![
            ChatMessage::System {
                content: "ignored".to_string(),
            },
            ChatMessage::Assistant {
                id: None,
                content: vec![
                    AssistantContent::text("  found the module  "),
                    AssistantContent::text("   "),
                    AssistantContent::ToolCall(ToolCall::new(
                        "call-0",
                        ToolFunction::new("grep", serde_json::json!({})),
                    )),
                ],
            },
            ChatMessage::User {
                content: vec![
                    UserContent::text("ignored user text"),
                    UserContent::ToolResult(DomainToolResult::new("call-1", "read", long_output)),
                    UserContent::ToolResult(DomainToolResult::new("call-2", "grep", "   ")),
                ],
            },
        ];

        let (texts, tools) = collect_history_findings(&history);

        assert_eq!(texts, vec!["found the module".to_string()]);
        assert_eq!(tools.len(), 1);
        assert!(tools[0].starts_with("Tool read: "));
        assert_eq!(tools[0].len(), "Tool read: ".len() + 200);
    }

    #[test]
    fn partial_result_reports_text_then_tools_then_nothing() {
        let text_history = vec![ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::text("partial answer")],
        }];
        let text_result = partial_result("reached turn limit of 3", &text_history);
        assert!(!text_result.is_error);
        assert_eq!(
            text_result.content,
            "Subagent reached turn limit of 3. Gathered findings:\n\npartial answer"
        );

        let tool_history = vec![ChatMessage::User {
            content: vec![UserContent::ToolResult(DomainToolResult::new(
                "call-1",
                "read",
                "file body",
            ))],
        }];
        let tool_result = partial_result("reached turn limit of 4", &tool_history);
        assert!(!tool_result.is_error);
        assert_eq!(
            tool_result.content,
            "Subagent reached turn limit of 4 during exploration. Discovered:\n\nTool read: file body"
        );

        let empty_result = partial_result("reached turn limit of 5", &[]);
        assert!(empty_result.is_error);
        assert_eq!(
            empty_result.content,
            "Subagent reached turn limit of 5 without producing output"
        );
    }
}
