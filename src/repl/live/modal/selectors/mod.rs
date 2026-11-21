pub mod auth;
pub mod help;
pub mod mcp;
pub mod model;
pub mod models;
pub mod search_engine;
pub mod session;
pub mod tools;

pub use auth::{handle_login_key, open_login_selector};
pub use help::{handle_help_key, open_help_selector};
pub use mcp::{handle_mcp_key, open_mcp_selector};
pub use model::{
    handle_guard_model_key, handle_model_key, open_guard_model_selector, open_model_selector,
    open_model_selector_with_default, open_slow_model_selector, open_smol_model_selector, refresh_open_model_selector,
};
pub use models::{handle_models_key, open_models_selector, update_models_role_description};
pub use search_engine::{handle_search_engine_key, open_search_engine_selector};
pub use session::{handle_session_key, open_session_selector};
pub use tools::{handle_tools_key, open_tools_selector, update_tools_search_engine};

/// Aligned selector row whose description carries the active check mark.
fn marked_option(id: &str, desc: &str, active: bool) -> crate::ui::interactive::ModalOption {
    let active_mark = if active { "  ✓" } else { "" };
    crate::ui::interactive::ModalOption::new(format!("{id:14}"), Some(format!("{desc}{active_mark}")))
}

#[cfg(test)]
pub(crate) mod test_support {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::ModalKeyResult;
    use crate::repl::ReplSession;
    use crate::ui::interactive::{InteractiveState, NullBackend, TerminalController};

    pub(crate) fn test_config(config_dir: &std::path::Path) -> rho_harness_core::config::Config {
        rho_harness_core::config::Config {
            config_dir: config_dir.to_path_buf(),
            sessions_dir: config_dir.join("sessions"),
            ..Default::default()
        }
    }

    pub(crate) fn create_test_session() -> ReplSession {
        let temp = tempfile::tempdir().unwrap();
        ReplSession::new(test_config(temp.path()), rho_engine::auth::AuthStore::default(), None)
    }

    pub(crate) fn open_in_test_controller(
        open: impl FnOnce(&ReplSession, &mut TerminalController<NullBackend>),
    ) -> TerminalController<NullBackend> {
        let session = create_test_session();
        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        open(&session, &mut controller);
        controller.redraw().unwrap();
        controller
    }

    pub(crate) fn assert_selector_opens_and_selects(
        open: impl FnOnce(&ReplSession, &mut TerminalController<NullBackend>),
        select: impl FnOnce(&mut TerminalController<NullBackend>, KeyEvent) -> crate::error::Result<ModalKeyResult>,
        title: &str,
        options_len: usize,
        expected: ModalKeyResult,
        closes_modal: bool,
    ) {
        let mut controller = open_in_test_controller(open);
        let modal = controller.state().active_modal().unwrap();
        assert_eq!(modal.title, title);
        assert_eq!(modal.options.len(), options_len);
        assert_eq!(modal.selected, 0);

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(select(&mut controller, enter).unwrap(), expected);
        assert_eq!(controller.state().active_modal().is_none(), closes_modal);
        controller.suspend().unwrap();
        controller.resume().unwrap();
    }
}
