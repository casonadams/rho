use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

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

pub fn handle_login_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        Some(ModalKeyResult::LoginProviderSelected {
            provider: opt.label.trim().to_string(),
        })
    })
}
