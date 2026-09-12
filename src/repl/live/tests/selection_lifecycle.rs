use super::common::HistoryTerminal;
use crate::ui::interactive::{
    InteractionInput, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse,
    InteractiveState, ModalMode, OptionLayout, TerminalController, UiEvent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::oneshot;

struct ModalDriver<'a> {
    controller: &'a mut TerminalController<HistoryTerminal>,
    pending: &'a mut Option<crate::repl::live::modal::PendingModal>,
}

impl ModalDriver<'_> {
    fn send(&mut self, code: KeyCode) {
        let _ = super::super::modal::handle_modal_key(
            self.controller,
            KeyEvent::new(code, KeyModifiers::NONE),
            self.pending,
        )
        .unwrap();
    }
    fn send_mod(&mut self, code: KeyCode, mods: KeyModifiers) {
        let _ =
            super::super::modal::handle_modal_key(self.controller, KeyEvent::new(code, mods), self.pending).unwrap();
    }
    fn install(&mut self, prompt: InteractionPrompt, tx: oneshot::Sender<InteractionResponse>) {
        super::super::modal::install_interaction(
            self.controller,
            UiEvent::Interaction {
                prompt,
                responder: InteractionResponder { responder: tx },
            },
            self.pending,
        );
    }
}

fn setup_searchable_modal() -> (TerminalController<HistoryTerminal>, tempfile::TempDir, usize) {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("in-progress user draft");
    let cursor = controller.state().editor().cursor();
    let temp = tempfile::tempdir().unwrap();
    let config = rho_harness_core::config::Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let session = crate::repl::ReplSession::new(config, crate::auth::AuthStore::default(), None);
    super::super::modal::open_model_selector(&session, &mut controller);
    (controller, temp, cursor)
}

#[test]
fn test_searchable_selection_typing_and_ctrl_c() {
    let (mut controller, _temp, _) = setup_searchable_modal();
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.send(KeyCode::Char('c'));
    assert_eq!(driver.controller.state().active_modal().unwrap().filter_query, "c");
    driver.send_mod(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(driver.controller.state().active_modal().unwrap().filter_query, "");
}

#[test]
fn test_searchable_selection_esc_restores_draft() {
    let (mut controller, _temp, cursor_before) = setup_searchable_modal();
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.send(KeyCode::Esc);
    assert!(driver.controller.state().active_modal().is_none());
    assert_eq!(
        (
            driver.controller.state().editor().text(),
            driver.controller.state().editor().cursor()
        ),
        ("in-progress user draft", cursor_before)
    );
}

fn sample_permission_prompt() -> InteractionPrompt {
    InteractionPrompt {
        title: "Perm".into(),
        body: "run".into(),
        options: vec![
            InteractionOption {
                label: "Allow".into(),
                description: None,
                input: None,
            },
            InteractionOption {
                label: "Deny".into(),
                description: None,
                input: Some(InteractionInput {
                    label: "reason".into(),
                    value: None,
                }),
            },
        ],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: OptionLayout::Vertical,
    }
}

#[test]
fn test_interaction_input_transition_and_escape_back() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (tx, _rx) = oneshot::channel();
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.install(sample_permission_prompt(), tx);

    driver.send(KeyCode::Down);
    driver.send(KeyCode::Enter);
    assert!(matches!(
        driver.controller.state().active_modal().unwrap().mode,
        ModalMode::Input { .. }
    ));

    driver.send(KeyCode::Char('s'));
    driver.send(KeyCode::Esc);
    assert!(matches!(
        driver.controller.state().active_modal().unwrap().mode,
        ModalMode::Select
    ));
}

#[test]
fn test_interaction_double_escape_cancels_and_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("prompt waiting to send");
    let (tx, mut rx) = oneshot::channel();
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.install(sample_permission_prompt(), tx);

    driver.send(KeyCode::Esc);
    assert!(
        driver.controller.state().active_modal().is_none()
            && driver.controller.state().editor().text() == "prompt waiting to send"
    );
    assert_eq!(rx.try_recv().unwrap(), InteractionResponse::Cancelled);
}

fn custom_input_prompt(label: &str, tag: &str, value: &str) -> InteractionPrompt {
    InteractionPrompt {
        title: "Input".into(),
        body: "Prompt:".into(),
        options: vec![InteractionOption {
            label: label.into(),
            description: None,
            input: Some(InteractionInput {
                label: tag.into(),
                value: Some(value.into()),
            }),
        }],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: OptionLayout::Vertical,
    }
}

#[test]
fn test_interaction_custom_input_submit_delivers_input_and_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("queued user question");
    let (tx, mut rx) = oneshot::channel();
    let prompt = custom_input_prompt("Custom", "tag", "v1.0.0");
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.install(prompt, tx);

    driver.send(KeyCode::Enter);
    assert_eq!(driver.controller.state().active_modal().unwrap().input.text(), "v1.0.0");

    driver.send(KeyCode::Enter);
    assert!(
        driver.controller.state().active_modal().is_none()
            && driver.controller.state().editor().text() == "queued user question"
    );
    assert_eq!(
        rx.try_recv().unwrap(),
        InteractionResponse::SelectedWithInput {
            index: 0,
            text: "v1.0.0".into()
        }
    );
}

#[test]
fn test_tree_and_settings_ctrl_c_dismiss_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("my unsent prompt");

    super::super::modal::open_settings_selector(None, None, &mut controller);
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.send_mod(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(
        driver.controller.state().active_modal().is_none()
            && driver.controller.state().editor().text() == "my unsent prompt"
    );

    let tree = rho_harness_core::session::tree::SessionTree::new();
    super::super::modal::open_tree_selector(&tree, driver.controller);
    driver.send_mod(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(
        driver.controller.state().active_modal().is_none()
            && driver.controller.state().editor().text() == "my unsent prompt"
    );
}

#[test]
fn test_interaction_custom_input_shift_enter_inserts_newline() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (tx, mut rx) = oneshot::channel();
    let prompt = custom_input_prompt("Edit", "cmd", "echo line1");
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    driver.install(prompt, tx);

    driver.send(KeyCode::Enter);
    driver.send_mod(KeyCode::Enter, KeyModifiers::SHIFT);
    driver.send(KeyCode::Char('2'));
    assert_eq!(
        driver.controller.state().active_modal().unwrap().input.text(),
        "echo line1\n2"
    );

    driver.send(KeyCode::Enter);
    assert!(driver.controller.state().active_modal().is_none());
    assert_eq!(
        rx.try_recv().unwrap(),
        InteractionResponse::SelectedWithInput {
            index: 0,
            text: "echo line1\n2".into()
        }
    );
}

#[test]
fn test_install_interaction_forwards_option_layout() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let (tx, _rx) = oneshot::channel();
    let mut pending = None;
    let mut driver = ModalDriver {
        controller: &mut controller,
        pending: &mut pending,
    };
    let mut prompt = sample_permission_prompt();
    prompt.option_layout = OptionLayout::Horizontal;
    driver.install(prompt, tx);

    let modal = driver.controller.state().active_modal().unwrap();
    assert_eq!(modal.option_layout, OptionLayout::Horizontal);
    assert_eq!(modal.body_scroll, 0);
}
