use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crate::ui::render::formatters::format_relative_time;

// =========================================================================
// Model Selector
// =========================================================================

fn is_default_model(session: &ReplSession, model_id: &str, provider: &str) -> bool {
    session.config.default_model.as_deref().is_some_and(|dm| {
        model_id == dm
            && session
                .config
                .default_provider
                .as_deref()
                .is_none_or(|dp| provider == dp)
    })
}

fn build_model_option(session: &ReplSession, item: &crate::repl::interactive::ModelItem) -> ModalOption {
    let active_mark = if item.id == session.config.model { "✓" } else { "" };
    let default_mark = if is_default_model(session, &item.id, &item.provider) {
        "default"
    } else {
        ""
    };
    ModalOption::new(
        item.id.clone(),
        Some(format!(
            "{}\t{}\t{}\t{}",
            item.provider, active_mark, default_mark, item.description
        )),
    )
}

pub fn open_model_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    open_model_selector_with_default(session, controller, false);
}

pub fn open_model_selector_with_default<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in discovered.iter().enumerate() {
        if item.id == session.config.model {
            initial_selection = i;
        }
        options.push(build_model_option(session, item));
    }

    let mut modal = ModalState::new("Select Model", "", options)
        .with_search(true)
        .with_save_as_default(save_as_default);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

fn extract_selected_model<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<(String, String)> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let selected_model = opt.label.clone();
    let provider = opt
        .description
        .as_deref()
        .and_then(|d| d.split('\t').next())
        .unwrap_or("anthropic")
        .to_string();
    Some((selected_model, provider))
}

fn pop_and_select_model<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    save_as_default: bool,
) -> Result<ModalKeyResult> {
    let selected = extract_selected_model(controller);
    controller.state_mut().pop_modal();
    controller.redraw()?;
    Ok(match selected {
        Some((model, provider)) => ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        },
        None => ModalKeyResult::Handled,
    })
}

pub fn handle_model_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    let save_as_default = controller.state().active_modal().is_some_and(|m| m.save_as_default);
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return pop_and_select_model(controller, true);
    }
    match key.code {
        KeyCode::Enter => pop_and_select_model(controller, save_as_default),
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

// =========================================================================
// Session Selector
// =========================================================================

pub fn open_session_selector<B: TerminalBackend>(sessions_dir: &Path, controller: &mut TerminalController<B>) {
    let summaries = rho_harness_core::session::list_session_summaries(sessions_dir).unwrap_or_default();
    let mut options = Vec::new();

    for item in summaries {
        let display_title = item.name.unwrap_or_else(|| item.session_id.clone());
        let relative_time = format_relative_time(item.last_modified);
        let desc = format!("{}\t{} turns\t{}", item.session_id, item.turn_count, item.preview);
        let label = format!("{display_title} ({relative_time})");
        options.push(ModalOption::new(label, Some(desc)));
    }

    let modal = ModalState::new("Resume Session", "", options).with_search(true);
    controller.state_mut().push_modal(modal);
}

fn selected_session_id<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<String> {
    let opt = controller.state().active_modal().and_then(|m| m.selected_option())?;
    let desc = opt.description.clone().unwrap_or_default();
    Some(desc.split('\t').next().unwrap_or(&desc).trim().to_string())
}

fn delete_selected_session<B: TerminalBackend>(controller: &mut TerminalController<B>) -> Result<ModalKeyResult> {
    let Some(session_id) = selected_session_id(controller) else {
        return Ok(ModalKeyResult::Handled);
    };
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.all_options.retain(|o| {
            let opt_desc = o.description.as_deref().unwrap_or("");
            opt_desc.split('\t').next().unwrap_or(opt_desc).trim() != session_id
        });
        let q = modal.filter_query.clone();
        modal.set_filter(&q);
    }
    controller.redraw()?;
    Ok(ModalKeyResult::SessionDeleted { session_id })
}

pub fn handle_session_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return delete_selected_session(controller);
    }
    super::dispatch_simple_selector(controller, key, |opt| {
        let desc = opt.description.as_deref().unwrap_or("");
        let session_id = desc.split('\t').next().unwrap_or(desc).trim().to_string();
        Some(ModalKeyResult::SessionSelected { session_id })
    })
}

// =========================================================================
// Login Provider Selector
// =========================================================================

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
    super::dispatch_simple_selector(controller, key, |opt| {
        Some(ModalKeyResult::LoginProviderSelected {
            provider: opt.label.trim().to_string(),
        })
    })
}

// =========================================================================
// MCP Server Selector
// =========================================================================

fn format_server_option(name: &str, server: &rho_harness_core::config::McpServerConfig) -> ModalOption {
    let transport_str = match server.resolved_transport() {
        rho_harness_core::config::McpTransportKind::Stdio => "stdio",
        rho_harness_core::config::McpTransportKind::StreamableHttp => "http",
        rho_harness_core::config::McpTransportKind::Sse => "sse",
    };

    let target = server
        .command
        .as_deref()
        .or(server.url.as_deref())
        .unwrap_or("<unspecified>");

    let active_mark = if server.enabled { "  ✓" } else { "  (off)" };
    let desc = format!("[{transport_str}] {target}{active_mark}");

    ModalOption::new(format!("{name:16}"), Some(desc))
}

fn build_mcp_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let mut options = Vec::new();
    for (name, server) in &session.config.mcp.servers {
        options.push(format_server_option(name, server));
    }

    if options.is_empty() {
        options.push(ModalOption::new(
            "none",
            Some("No MCP servers configured (add to config.toml or .mcp.json)".to_string()),
        ));
    }

    (options, 0)
}

pub fn open_mcp_selector<B: TerminalBackend>(session: &ReplSession, controller: &mut TerminalController<B>) {
    let (options, initial_selection) = build_mcp_options(session);
    let mut modal = ModalState::new("Model Context Protocol", "", options).with_search(true);
    modal.selected = initial_selection;
    controller.state_mut().push_modal(modal);
}

pub fn handle_mcp_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    super::dispatch_simple_selector(controller, key, |opt| {
        let raw = opt.label.trim();
        (!raw.eq_ignore_ascii_case("none")).then(|| ModalKeyResult::McpServerToggled {
            server: raw.to_string(),
        })
    })
}

// =========================================================================
// Help Selector
// =========================================================================

fn build_help_options() -> Vec<ModalOption> {
    vec![
        ModalOption::new(
            format!("{:<15}", "/settings"),
            Some("Interactive runtime interface settings"),
        ),
        ModalOption::new(format!("{:<15}", "/model"), Some("Inspect or switch the active model")),
        ModalOption::new(format!("{:<15}", "/resume"), Some("Resume a prior session")),
        ModalOption::new(
            format!("{:<15}", "/session"),
            Some("Token capacity & diagnostics (alias: /tokens)"),
        ),
        ModalOption::new(
            format!("{:<15}", "/compact"),
            Some("Summarize earlier context to free space"),
        ),
        ModalOption::new(
            format!("{:<15}", "/tree"),
            Some("View conversation turn and branch tree"),
        ),
        ModalOption::new(
            format!("{:<15}", "/fork"),
            Some("Fork session from turn into a new session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/clone"),
            Some("Duplicate active branch into a new session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/name"),
            Some("Assign a human-readable name to session"),
        ),
        ModalOption::new(
            format!("{:<15}", "/rewind"),
            Some("Rewind context to a specific prior turn"),
        ),
        ModalOption::new(
            format!("{:<15}", "/clear"),
            Some("Start a new session; preserve history (alias: /new)"),
        ),
        ModalOption::new(format!("{:<15}", "/mcp"), Some("List configured MCP servers")),
        ModalOption::new(format!("{:<15}", "/login"), Some("Add API-key or subscription auth")),
        ModalOption::new(format!("{:<15}", "/logout"), Some("Remove stored provider auth")),
        ModalOption::new(format!("{:<15}", "/skill"), Some("List or inspect skills")),
        ModalOption::new(
            format!("{:<15}", "/reload"),
            Some("Re-read config, skills, and MCP tools"),
        ),
        ModalOption::new(
            format!("{:<15}", "/export"),
            Some("Export active branch as HTML or Markdown"),
        ),
        ModalOption::new(
            format!("{:<15}", "/remote"),
            Some("Pair session with web dashboard via Iroh P2P"),
        ),
        ModalOption::new(format!("{:<15}", "/exit"), Some("Exit rho (alias: /quit)")),
        ModalOption::new(format!("{:<15}", "Tab"), Some("Complete slash commands & skill names")),
        ModalOption::new(format!("{:<15}", "Shift+Tab"), Some("Cycle thinking level")),
        ModalOption::new(format!("{:<15}", "Ctrl+L"), Some("Select model")),
        ModalOption::new(format!("{:<15}", "Ctrl+O"), Some("Expand or collapse tool output")),
        ModalOption::new(format!("{:<15}", "Ctrl+T"), Some("Toggle thinking blocks visibility")),
        ModalOption::new(format!("{:<15}", "Ctrl+C"), Some("Clear the input prompt")),
        ModalOption::new(format!("{:<15}", "Ctrl+D"), Some("Exit at an empty prompt")),
        ModalOption::new(
            format!("{:<15}", "Escape"),
            Some("Cancel active execution or dismiss modal"),
        ),
    ]
}

pub fn open_help_selector<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let options = build_help_options();
    let modal = ModalState::new("Help", "", options).with_search(true);
    controller.state_mut().push_modal(modal);
}

pub fn handle_help_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    super::dispatch_simple_selector(controller, key, |opt| {
        let raw = opt.label.trim();
        raw.starts_with('/').then(|| ModalKeyResult::HelpCommandSelected {
            command: raw.to_string(),
        })
    })
}

// =========================================================================
// Remote Access Modal
// =========================================================================

pub fn open_remote_modal<B: TerminalBackend>(controller: &mut TerminalController<B>, pairing_url: &str) {
    let options = vec![
        ModalOption::new(
            format!("{:<15}", "Copy Link"),
            Some("Copy web pairing URL to system clipboard"),
        ),
        ModalOption::new(
            format!("{:<15}", "Show QR Code"),
            Some("Print terminal QR code into transcript"),
        ),
        ModalOption::new(
            format!("{:<15}", "Dismiss"),
            Some("Close this modal (session stays shared)"),
        ),
    ];
    let modal = ModalState::new("Remote Access", pairing_url, options);
    controller.state_mut().push_modal(modal);
}

pub fn handle_remote_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    if matches!(
        (key.modifiers, key.code),
        (KeyModifiers::NONE, KeyCode::Char('c' | 'C'))
    ) {
        let url = controller
            .state()
            .active_modal()
            .map(|m| m.body.clone())
            .unwrap_or_default();
        super::pop_and_cancel(controller)?;
        if !url.is_empty() {
            let _ = crate::platform::clipboard::set_text(&url);
            controller.set_system_message("Copied pairing URL to clipboard");
            controller.redraw()?;
        }
        return Ok(ModalKeyResult::Handled);
    }

    match key.code {
        KeyCode::Enter => {
            let selected = controller
                .state()
                .active_modal()
                .and_then(|m| m.selected_option())
                .map(|o| o.label.trim().to_string())
                .unwrap_or_default();
            let url = controller
                .state()
                .active_modal()
                .map(|m| m.body.clone())
                .unwrap_or_default();
            super::pop_and_cancel(controller)?;
            if selected.starts_with("Copy Link") && !url.is_empty() {
                let _ = crate::platform::clipboard::set_text(&url);
                controller.set_system_message("Copied pairing URL to clipboard");
            } else if selected.starts_with("Show QR Code") && !url.is_empty() {
                controller.set_system_message("Pairing QR code generated");
            }
            controller.redraw()?;
            Ok(ModalKeyResult::Handled)
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
