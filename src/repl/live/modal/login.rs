use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;

const PROVIDER_DEFS: &[(&str, &str)] = &[
    ("antigravity", "Google Cloud Code Assist (OAuth)"),
    ("chatgpt", "ChatGPT Plus/Pro subscription (OAuth)"),
    ("claude", "Claude Pro/Max subscription (OAuth)"),
    ("copilot", "GitHub Copilot subscription (OAuth)"),
    ("openrouter", "OpenRouter universal gateway (OAuth / Key)"),
    ("anthropic", "Anthropic Claude models (API Key)"),
    ("openai", "OpenAI GPT & reasoning models (API Key)"),
    ("gemini", "Google Gemini models (API Key)"),
    ("deepseek", "DeepSeek models (API Key)"),
    ("groq", "Groq fast inference (API Key)"),
    ("mistral", "Mistral and Codestral models (API Key)"),
    ("xai", "xAI Grok models (API Key)"),
    ("cohere", "Cohere Command models (API Key)"),
    ("ollama-cloud", "Hosted open models (API Key)"),
];

fn format_provider_option(id: &str, desc: &str, configured: &[String]) -> ModalOption {
    let active = configured.iter().any(|p| p.eq_ignore_ascii_case(id));
    let active_mark = if active { "  ✓" } else { "" };
    ModalOption::new(format!("{id:14}"), Some(format!("{desc}{active_mark}")))
}

fn append_custom_providers(options: &mut Vec<ModalOption>, session: &ReplSession, configured: &[String]) {
    for custom_name in session.config.providers.keys() {
        if !PROVIDER_DEFS.iter().any(|(id, _)| id == custom_name) {
            options.push(format_provider_option(
                custom_name,
                "Configured provider (API Key)",
                configured,
            ));
        }
    }
}

fn build_login_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let configured = session.auth_store.list_configured_providers();
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, &(id, desc)) in PROVIDER_DEFS.iter().enumerate() {
        if id.eq_ignore_ascii_case(&session.config.provider) {
            initial_selection = i;
        }
        options.push(format_provider_option(id, desc, &configured));
    }

    append_custom_providers(&mut options, session, &configured);
    (options, initial_selection)
}

pub fn open_login_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let (options, initial_selection) = build_login_options(session);
    let mut modal = ModalState::new("Login Provider", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_provider<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    Some(opt.label.trim().to_string())
}

fn pop_and_select<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let selected = extract_selected_provider(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some(provider) => ModalKeyResult::LoginProviderSelected { provider },
        None => ModalKeyResult::Handled,
    })
}

fn pop_and_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn apply_login_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    character: Option<char>,
) -> Result<ModalKeyResult> {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        let mut query = modal.filter_query.clone();
        if let Some(c) = character {
            query.push(c);
        } else {
            query.pop();
        }
        modal.set_filter(&query);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::Handled)
}

fn clear_filter_or_cancel<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let has_filter = controller
        .state()
        .active_modal()
        .is_some_and(|m| !m.filter_query.is_empty());
    if has_filter {
        if let Some(modal) = controller.state_mut().active_modal_mut() {
            modal.set_filter("");
        }
        controller.redraw()?;
        return Ok(ModalKeyResult::Handled);
    }
    pop_and_cancel(controller)
}

fn handle_nav_or_filter<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            controller.state_mut().select_previous_modal_option();
            controller.redraw()?;
        }
        KeyCode::Down | KeyCode::Tab => {
            controller.state_mut().select_next_modal_option();
            controller.redraw()?;
        }
        KeyCode::Backspace => return apply_login_filter(controller, None),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return clear_filter_or_cancel(controller);
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            return apply_login_filter(controller, Some(c));
        }
        _ => {}
    }
    Ok(ModalKeyResult::Handled)
}

pub fn handle_login_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => pop_and_select(controller),
        KeyCode::Esc => pop_and_cancel(controller),
        _ => handle_nav_or_filter(controller, &key),
    }
}
