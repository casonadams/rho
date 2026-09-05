use super::common::HistoryTerminal;
use crate::ui::interactive::{
    InteractionInput, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse,
    InteractiveState, ModalMode, TerminalController, UiEvent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::oneshot;

#[test]
fn test_searchable_selection_typing_ctrl_c_and_esc_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("in-progress user draft");
    let cursor_before = controller.state().editor().cursor();

    let temp = tempfile::tempdir().unwrap();
    let config = rho_harness_core::config::Config {
        config_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let auth_store = crate::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);

    super::super::modal::open_model_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");
    controller.redraw().unwrap();

    let rendered = controller.rendered().expect("rendered frame");
    assert!(rendered.lines.iter().any(|l| l.contains("Select Model")));
    assert!(rendered.lines.iter().any(|l| l.contains("Draft:")));

    let key_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, key_c, &mut None).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "c");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let _ = super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert!(controller.state().active_modal().is_some());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");

    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, esc, &mut None).unwrap();
    assert!(controller.state().active_modal().is_none());
    assert_eq!(controller.state().editor().text(), "in-progress user draft");
    assert_eq!(controller.state().editor().cursor(), cursor_before);
}

#[test]
fn test_interaction_custom_input_transition_and_escape_back_preserves_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("prompt waiting to send");
    let (responder_tx, mut responder_rx) = oneshot::channel();

    let prompt = InteractionPrompt {
        title: "Permission Required".to_string(),
        body: "tool bash: rm -rf /tmp/target".to_string(),
        options: vec![
            InteractionOption {
                label: "Allow".to_string(),
                description: None,
                input: None,
            },
            InteractionOption {
                label: "Deny with reason".to_string(),
                description: None,
                input: Some(InteractionInput {
                    label: "reason".to_string(),
                    value: None,
                }),
            },
        ],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
    };

    let mut pending = None;
    super::super::modal::install_interaction(
        &mut controller,
        UiEvent::Interaction {
            prompt,
            responder: InteractionResponder {
                responder: responder_tx,
            },
        },
        &mut pending,
    );
    controller.redraw().unwrap();

    assert!(controller.state().active_modal().is_some());
    let rendered = controller.rendered().expect("rendered frame");
    assert!(rendered.lines.iter().any(|l| l.contains("Draft:")));

    let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, down_key, &mut pending).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut pending).unwrap();

    let active_modal = controller.state().active_modal().unwrap();
    assert!(matches!(active_modal.mode, ModalMode::Input { .. }));

    let char_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, char_s, &mut pending).unwrap();
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "s");

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, esc_key, &mut pending).unwrap();
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        ModalMode::Select
    ));

    let _ = super::super::modal::handle_modal_key(&mut controller, esc_key, &mut pending).unwrap();
    assert!(controller.state().active_modal().is_none());
    assert_eq!(controller.state().editor().text(), "prompt waiting to send");

    let response = responder_rx.try_recv().unwrap();
    assert_eq!(response, InteractionResponse::Cancelled);
}

#[test]
fn test_interaction_custom_input_submit_delivers_input_and_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("queued user question");
    let (responder_tx, mut responder_rx) = oneshot::channel();

    let prompt = InteractionPrompt {
        title: "Input Required".to_string(),
        body: "Enter deployment tag:".to_string(),
        options: vec![InteractionOption {
            label: "Custom Tag".to_string(),
            description: None,
            input: Some(InteractionInput {
                label: "tag".to_string(),
                value: Some("v1.0.0".to_string()),
            }),
        }],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
    };

    let mut pending = None;
    super::super::modal::install_interaction(
        &mut controller,
        UiEvent::Interaction {
            prompt,
            responder: InteractionResponder {
                responder: responder_tx,
            },
        },
        &mut pending,
    );

    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let _ = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut pending).unwrap();
    assert!(matches!(
        controller.state().active_modal().unwrap().mode,
        ModalMode::Input { .. }
    ));
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "v1.0.0");

    let _ = super::super::modal::handle_modal_key(&mut controller, enter_key, &mut pending).unwrap();
    assert!(controller.state().active_modal().is_none());
    assert_eq!(controller.state().editor().text(), "queued user question");

    let response = responder_rx.try_recv().unwrap();
    match response {
        InteractionResponse::SelectedWithInput { index, text } => {
            assert_eq!(index, 0);
            assert_eq!(text, "v1.0.0");
        }
        other => panic!("expected SelectedWithInput, got {other:?}"),
    }
}

#[test]
fn test_tree_and_settings_ctrl_c_dismiss_restores_draft() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("my unsent prompt");

    super::super::modal::open_settings_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Settings");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let _ = super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert!(controller.state().active_modal().is_none());
    assert_eq!(controller.state().editor().text(), "my unsent prompt");

    let tree = rho_harness_core::session::tree::SessionTree::new();
    super::super::modal::open_tree_selector(&tree, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Conversation Tree");

    let _ = super::super::modal::handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert!(controller.state().active_modal().is_none());
    assert_eq!(controller.state().editor().text(), "my unsent prompt");
}
