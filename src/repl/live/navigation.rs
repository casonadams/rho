use std::collections::HashMap;

use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::repl::model_switch::{next_listed_route, print_session_model, report_model_update, switch_session_model};
use crate::ui::interactive::{
    Activity, InteractiveState, TerminalBackend, TerminalController, ToolItem, TranscriptItem,
};
use rho_harness_core::thinking::ThinkingLevel;

pub fn copy_last_message<B: TerminalBackend>(session: &ReplSession, controller: &TerminalController<B>) {
    let last_text = controller.transcript().iter().rev().find_map(|item| match item {
        TranscriptItem::AssistantText(text) => Some(text.clone()),
        _ => None,
    });

    if let Some(text) = last_text {
        if crate::platform::clipboard::set_text(&text).is_ok() {
            session.renderer.print_status("Copied message to clipboard");
        } else {
            session.renderer.print_status("Failed to access clipboard");
        }
    } else {
        session.renderer.print_status("No assistant message to copy");
    }
}

pub async fn paste_clipboard_async<B: TerminalBackend>(
    _renderer: &crate::ui::TerminalRenderer,
    controller: &mut TerminalController<B>,
) {
    if let Some(text) = crate::platform::clipboard::get_text_or_image_path_async().await
        && !crate::repl::live::modal::handle_modal_paste(controller, &text)
    {
        controller.state_mut().editor_mut().handle_paste(&text);
    }
}

fn cached_hostname() -> Option<String> {
    static HOSTNAME: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
        std::env::var("HOSTNAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::fs::read_to_string("/etc/hostname")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
    });
    HOSTNAME.clone()
}

pub fn update_footer(state: &mut InteractiveState, session: &ReplSession, engine: &AgentEngine) {
    state.set_active_tool(None);
    let footer = state.footer_mut();
    footer.activity = Activity::Idle;
    footer.running_tool = None;
    footer.provider = session.config.provider.clone();
    footer.model = session.config.model.clone();
    footer.thinking_level = session.config.thinking_level.clone();

    let cwd = std::env::current_dir().unwrap_or_default();
    footer.cwd = Some(cwd.display().to_string());
    footer.git_branch = crate::ui::interactive::footer::get_git_branch(&cwd);
    footer.session_name = engine.session_manager.cached_session_name();
    footer.quota = engine.quota_display();
    footer.context_percent = engine.context_percent_f64();
    footer.context_window = engine.context_limit();

    let totals = engine.session_usage_totals();
    footer.total_input_tokens = totals.total_input;
    footer.total_output_tokens = totals.total_output;
    footer.total_cache_read_tokens = totals.total_cache_read;
    footer.total_cache_write_tokens = totals.total_cache_write;
    footer.tokens_per_second = engine.tokens_per_second();
    footer.context = Some(engine.context_remaining_display());
    footer.show_label = session.config.show_label;
    footer.prompt_config = session.config.ui.prompt.clone();
    footer.status_bar_config = session.config.ui.status_bar.clone();
    footer.session_id = Some(engine.session_manager.session_id.clone());
    let status_bar = session.config.ui.status_bar.as_ref();
    footer.hostname = status_bar
        .is_some_and(|c| {
            crate::ui::interactive::footer::segments::status_bar_uses_segment(
                c,
                rho_harness_core::config::StatusBarSegmentId::Hostname,
            )
        })
        .then(cached_hostname)
        .flatten();
    footer.git_status = status_bar
        .is_some_and(|c| {
            crate::ui::interactive::footer::segments::status_bar_uses_segment(
                c,
                rho_harness_core::config::StatusBarSegmentId::Git,
            )
        })
        .then(|| crate::ui::interactive::footer::path::cached_git_status_counts(&cwd))
        .flatten();
    footer.git_pr = status_bar
        .is_some_and(|c| {
            crate::ui::interactive::footer::segments::status_bar_uses_segment(
                c,
                rho_harness_core::config::StatusBarSegmentId::Pr,
            )
        })
        .then(|| crate::ui::interactive::footer::cached_git_pr(&cwd, footer.git_branch.as_deref()))
        .flatten();
}

pub async fn cycle_thinking_level<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    controller: &mut TerminalController<B>,
) {
    let next_level = ThinkingLevel::from_setting(session.config.thinking_level.as_deref()).next();
    session.config.thinking_level = next_level.setting();

    engine.config.thinking_level = session.config.thinking_level.clone();
    let result = engine.update_model().await;
    report_model_update(session, engine, result, "Could not update thinking level");

    update_footer(controller.state_mut(), session, engine);
    session
        .renderer
        .print_status(&format!("Thinking: {}", next_level.as_str()));
}

pub struct ModelCycleContext<'a, 'b, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'a mut TerminalController<B>,
}

pub async fn cycle_model<B: TerminalBackend>(ctx: &mut ModelCycleContext<'_, '_, B>, direction: i32) {
    let Some(route) = next_listed_route(ctx.session, direction) else {
        return;
    };
    let switched = switch_session_model(&route, ctx.session, ctx.engine).await;
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    if switched {
        print_session_model(ctx.session);
    }
}

pub fn record_submission(history: &mut InteractiveHistory, text: &str) -> crate::error::Result<()> {
    history
        .record(text)
        .map_err(|e| anyhow::anyhow!("History could not be updated: {e}"))?;
    Ok(())
}

pub fn navigate_history_previous<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_up(width) {
        return true;
    }
    let Some(value) = history.previous(controller.state().editor().text()) else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

pub fn navigate_history_next<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_down(width) {
        return true;
    }
    let Some(value) = history.next_entry() else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

pub fn apply_completion<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) -> bool {
    apply_completion_generic(controller, completions)
}

pub fn apply_completion_generic<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) -> bool {
    let editor = controller.state().editor();
    let text = editor.text();
    let byte_index = editor.cursor();
    let candidates = completions.complete(text, byte_index);
    let Some(first) = candidates.first() else {
        return false;
    };
    let mut updated = String::new();
    updated.push_str(&text[..first.replacement.start]);
    updated.push_str(&first.value);
    if !first.value.ends_with(' ') {
        updated.push(' ');
    }
    updated.push_str(&text[first.replacement.end..]);
    controller.state_mut().editor_mut().set_text(&updated);
    true
}

pub fn restore_queued_messages<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let mut restored = Vec::new();
    while let Some(message) = controller.state_mut().pop_queued() {
        restored.push(message.text);
    }
    if !restored.is_empty() {
        controller.state_mut().editor_mut().set_text(restored.join("\n\n"));
    }
}

fn tool_result_text(result: &rho_harness_core::model::ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|part| part.as_text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn apply_tool_result(
    pending: &mut HashMap<String, ToolItem>,
    result: &rho_harness_core::model::ToolResult,
) -> ToolItem {
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
            file_change: None,
        },
    }
}

fn push_user_content(
    items: &mut Vec<TranscriptItem>,
    pending: &mut HashMap<String, ToolItem>,
    content: &[rho_harness_core::model::UserContent],
) {
    for item in content {
        match item {
            rho_harness_core::model::UserContent::Text(t) => {
                if !t.text.trim().is_empty() {
                    items.push(TranscriptItem::UserMessage(t.text.clone()));
                }
            }
            rho_harness_core::model::UserContent::ToolResult(result) => {
                let tool = apply_tool_result(pending, result);
                if crate::ui::interactive::RunningTool::has_transcript_card(&tool.name) {
                    items.push(TranscriptItem::tool(tool));
                }
            }
            _ => {}
        }
    }
}

fn push_assistant_content(
    items: &mut Vec<TranscriptItem>,
    pending: &mut HashMap<String, ToolItem>,
    content: &[rho_harness_core::model::AssistantContent],
) {
    for item in content {
        match item {
            rho_harness_core::model::AssistantContent::Text(t) => {
                if !t.text.trim().is_empty() {
                    items.push(TranscriptItem::AssistantText(t.text.clone()));
                }
            }
            rho_harness_core::model::AssistantContent::ToolCall(call) => {
                let tool = ToolItem {
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                    is_error: false,
                    output: String::new(),
                    output_summary: String::new(),
                    duration_ms: None,
                    file_change: None,
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
) -> std::io::Result<()> {
    let mut items = Vec::new();
    let mut pending_tools: HashMap<String, ToolItem> = HashMap::new();

    for message in tree.active_messages() {
        match message {
            rho_harness_core::model::ChatMessage::User { content } => {
                push_user_content(&mut items, &mut pending_tools, content.as_slice());
            }
            rho_harness_core::model::ChatMessage::Assistant { content, .. } => {
                push_assistant_content(&mut items, &mut pending_tools, content.as_slice());
            }
            _ => {}
        }
    }

    for (_, tool) in pending_tools {
        items.push(TranscriptItem::tool(tool));
    }

    controller.set_transcript(items)
}
