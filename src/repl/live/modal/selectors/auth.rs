use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use rho_harness_core::provider::{Connection, ProviderId};
use std::str::FromStr;

fn format_provider_option(id: &str, desc: &str, configured: &[String]) -> ModalOption {
    let logged_in = configured.iter().any(|p| Connection::parse(p).provider == id);
    super::marked_option(id, desc, logged_in)
}

fn append_custom_providers(options: &mut Vec<ModalOption>, session: &ReplSession, configured: &[String]) {
    for custom_name in session.config.providers.keys() {
        if ProviderId::from_str(custom_name).is_err() {
            options.push(format_provider_option(
                custom_name,
                "Configured provider (API key)",
                configured,
            ));
        }
    }
}

/// The provider's description and login mode, plus each signed-in account's email.
fn login_description(session: &ReplSession, provider: ProviderId) -> String {
    let info = provider.info();
    let desc = format!("{} ({})", info.description, provider.auth_mode_label());
    let logins = session.auth_store.logins(provider);
    let emails: Vec<&str> = logins.iter().filter_map(|login| login.email.as_deref()).collect();
    if emails.is_empty() {
        desc
    } else {
        format!("{desc} · {}", emails.join(", "))
    }
}

fn build_login_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let configured = session.auth_store.list_configured_providers();
    let active_provider = session.config.route().connection.provider;
    let mut options = Vec::new();
    let mut initial_selection = 0;

    let providers = ProviderId::ALL.into_iter().filter(|&id| id != ProviderId::Local);
    for (i, provider) in providers.enumerate() {
        if provider.as_str() == active_provider {
            initial_selection = i;
        }
        options.push(format_provider_option(
            provider.as_str(),
            &login_description(session, provider),
            &configured,
        ));
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
