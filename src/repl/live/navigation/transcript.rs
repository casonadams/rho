use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController, ToolItem, TranscriptItem};
use std::collections::HashMap;

fn tool_result_text(result: &rig::message::ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|part| part.as_text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn apply_tool_result(pending: &mut HashMap<String, ToolItem>, result: &rig::message::ToolResult) -> ToolItem {
    let text = tool_result_text(result);
    match pending.remove(result.call.as_str()) {
        Some(mut tool) => {
            tool.output = text.clone();
            tool.output_summary = text;
            tool
        }
        None => ToolItem {
            name: "tool".into(),
            arguments: serde_json::Value::Null,
            is_error: false,
            output: text.clone(),
            output_summary: text,
            duration_ms: None,
        },
    }
}

fn push_user_content(
    (items, pending, history): (
        &mut Vec<TranscriptItem>,
        &mut HashMap<String, ToolItem>,
        &mut InteractiveHistory,
    ),
    content: &[rig::message::UserContent],
) {
    for item in content {
        match item {
            rig::message::UserContent::Text(t) => {
                if !t.text.trim().is_empty() {
                    let _ = history.record(&t.text);
                    items.push(TranscriptItem::UserMessage(t.text.clone()));
                }
            }
            rig::message::UserContent::ToolResult(result) => {
                let tool = apply_tool_result(pending, result);
                items.push(TranscriptItem::Tool(tool));
            }
            _ => {}
        }
    }
}

fn push_assistant_content(
    (items, pending): (&mut Vec<TranscriptItem>, &mut HashMap<String, ToolItem>),
    content: &[rig::message::AssistantContent],
) {
    for item in content {
        match item {
            rig::message::AssistantContent::Text(t) => {
                if !t.text.trim().is_empty() {
                    items.push(TranscriptItem::AssistantText(t.text.clone()));
                }
            }
            rig::message::AssistantContent::ToolCall(call) => {
                let tool = ToolItem {
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                    is_error: false,
                    output: String::new(),
                    output_summary: String::new(),
                    duration_ms: None,
                };
                pending.insert(call.id.to_string(), tool);
            }
            _ => {}
        }
    }
}

pub fn hydrate_session_transcript<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    tree: &rho_harness_core::session::tree::SessionTree,
    history: &mut InteractiveHistory,
) -> std::io::Result<()> {
    let mut items = Vec::new();
    let mut pending_tools: HashMap<String, ToolItem> = HashMap::new();

    for message in tree.active_messages() {
        match message {
            rig::message::Message::User { content } => {
                push_user_content((&mut items, &mut pending_tools, history), content.as_slice());
            }
            rig::message::Message::Assistant { content, .. } => {
                push_assistant_content((&mut items, &mut pending_tools), content.as_slice());
            }
            _ => {}
        }
    }

    for (_, tool) in pending_tools {
        items.push(TranscriptItem::Tool(tool));
    }

    controller.set_transcript(items)
}
