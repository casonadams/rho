use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

fn build_login_options(session: &ReplSession) -> (Vec<ModalOption>, usize) {
    let active_provider = session.config.route().connection.provider;
    let choices = crate::cli::auth::login_choices(&session.config, &session.auth_store);
    let initial_selection = choices.iter().position(|c| c.name == active_provider).unwrap_or(0);
    let options = choices
        .iter()
        .map(|c| super::marked_option(&c.name, &c.description, c.signed_in))
        .collect();
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
