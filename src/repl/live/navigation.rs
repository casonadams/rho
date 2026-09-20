use std::collections::HashMap;

use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{
    Activity, InteractiveState, TerminalBackend, TerminalController, ToolItem, TranscriptItem,
};

pub const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

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
    footer.context_window = engine.context_limit().unwrap_or(0);

    let totals = engine.session_usage_totals();
    footer.total_input_tokens = totals.total_input;
    footer.total_output_tokens = totals.total_output;
    footer.total_cache_read_tokens = totals.total_cache_read;
    footer.total_cache_write_tokens = totals.total_cache_write;
    footer.tokens_per_second = engine.tokens_per_second();
    footer.context = Some(engine.context_remaining_display());
    footer.show_label = session.config.show_label;
    footer.remote_active = crate::platform::remote::is_remote_active();
    footer.remote_peers = crate::platform::remote::remote_peer_count();
}

pub async fn cycle_thinking_level<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    controller: &mut TerminalController<B>,
) {
    let current = session.config.thinking_level.as_deref().unwrap_or("off");
    let current_idx = THINKING_LEVELS
        .iter()
        .position(|&l| l.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    let next_idx = (current_idx + 1) % THINKING_LEVELS.len();
    let next_level = THINKING_LEVELS[next_idx];

    session.config.thinking_level = if next_level == "off" {
        None
    } else {
        Some(next_level.to_string())
    };

    engine.config.thinking_level = session.config.thinking_level.clone();
    if let Err(err) = engine.update_model().await {
        session
            .renderer
            .print_notice(&format!("\nWarning: Could not update thinking level: {err}\n"));
    }

    update_footer(controller.state_mut(), session, engine);
    session.renderer.print_status(&format!(
        "Thinking: {}",
        session.config.thinking_level.as_deref().unwrap_or("off")
    ));
}

pub struct ModelCycleContext<'a, 'b, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'a mut TerminalController<B>,
}

fn next_model_index(current_idx: usize, len: usize, direction: i32) -> usize {
    if direction >= 0 {
        (current_idx + 1) % len
    } else if current_idx == 0 {
        len - 1
    } else {
        current_idx - 1
    }
}

pub async fn cycle_model<B: TerminalBackend>(ctx: &mut ModelCycleContext<'_, '_, B>, direction: i32) {
    let models = crate::repl::interactive::discover_models(&ctx.session.config, &ctx.session.auth_store);
    if models.is_empty() {
        return;
    }
    let current_idx = models
        .iter()
        .position(|m| m.id == ctx.session.config.model)
        .unwrap_or(0);
    let item = &models[next_model_index(current_idx, models.len(), direction)];
    ctx.session.config.model = item.id.clone();
    ctx.session.config.provider = item.provider.clone();

    if let Err(err) = ctx.engine.switch_model(&item.id, &item.provider).await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
    }

    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.session.renderer.print_status(&format!(
        "Model: {} ({})",
        ctx.session.config.model, ctx.session.config.provider
    ));
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
    items: &mut Vec<TranscriptItem>,
    pending: &mut HashMap<String, ToolItem>,
    history: &mut InteractiveHistory,
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
    items: &mut Vec<TranscriptItem>,
    pending: &mut HashMap<String, ToolItem>,
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
                push_user_content(&mut items, &mut pending_tools, history, content.as_slice());
            }
            rig::message::Message::Assistant { content, .. } => {
                push_assistant_content(&mut items, &mut pending_tools, content.as_slice());
            }
            _ => {}
        }
    }

    for (_, tool) in pending_tools {
        items.push(TranscriptItem::Tool(tool));
    }

    controller.set_transcript(items)
}
