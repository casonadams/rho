use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub fn hydrate_session_transcript<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    tree: &rho_harness_core::session::tree::SessionTree,
    history: &mut InteractiveHistory,
) -> std::io::Result<()> {
    let mut items = Vec::new();
    let mut pending_tools: std::collections::HashMap<String, crate::ui::interactive::ToolItem> =
        std::collections::HashMap::new();

    for message in tree.active_messages() {
        match message {
            rig::message::Message::User { content } => {
                for item in content {
                    match item {
                        rig::message::UserContent::Text(t) => {
                            if !t.text.trim().is_empty() {
                                let _ = history.record(&t.text);
                                items.push(crate::ui::interactive::TranscriptItem::UserMessage(t.text));
                            }
                        }
                        rig::message::UserContent::ToolResult(result) => {
                            let text = result
                                .content
                                .iter()
                                .filter_map(|part| part.as_text())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let tool = if let Some(mut tool) = pending_tools.remove(result.call.as_str()) {
                                tool.output = text.clone();
                                tool.output_summary = text;
                                tool
                            } else {
                                crate::ui::interactive::ToolItem {
                                    name: "tool".into(),
                                    arguments: serde_json::Value::Null,
                                    is_error: false,
                                    output: text.clone(),
                                    output_summary: text,
                                    duration_ms: None,
                                }
                            };
                            items.push(crate::ui::interactive::TranscriptItem::Tool(tool));
                        }
                        _ => {}
                    }
                }
            }
            rig::message::Message::Assistant { content, .. } => {
                for item in content {
                    match item {
                        rig::message::AssistantContent::Text(t) => {
                            if !t.text.trim().is_empty() {
                                items.push(crate::ui::interactive::TranscriptItem::AssistantText(t.text));
                            }
                        }
                        rig::message::AssistantContent::ToolCall(call) => {
                            let tool = crate::ui::interactive::ToolItem {
                                name: call.function.name.clone(),
                                arguments: call.function.arguments.clone(),
                                is_error: false,
                                output: String::new(),
                                output_summary: String::new(),
                                duration_ms: None,
                            };
                            pending_tools.insert(call.id.to_string(), tool);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    for (_, tool) in pending_tools {
        items.push(crate::ui::interactive::TranscriptItem::Tool(tool));
    }

    controller.set_transcript(items)
}
