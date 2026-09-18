use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent};

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

pub fn handle_login_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    match key.code {
        KeyCode::Enter => {
            let provider = extract_selected_provider(controller);
            super::pop_and_cancel(controller)?;
            Ok(match provider {
                Some(provider) => ModalKeyResult::LoginProviderSelected { provider },
                None => ModalKeyResult::Handled,
            })
        }
        KeyCode::Esc => {
            super::pop_and_cancel(controller)?;
            Ok(ModalKeyResult::Handled)
        }
        _ => {
            super::handle_selector_nav(controller, &key)?;
            Ok(ModalKeyResult::Handled)
        }
    }
}
