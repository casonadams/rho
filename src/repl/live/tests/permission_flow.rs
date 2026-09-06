use super::common::HistoryTerminal;
use crate::ui::interactive::{
    InteractionPrompt, InteractionResponder, InteractionResponse, InteractiveState, OptionLayout, TerminalController,
    UiEvent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_engine::permission::prompt::build_permission_prompt;
use serde_json::json;
use tokio::sync::oneshot;

struct PermDriver {
    controller: TerminalController<HistoryTerminal>,
    pending: Option<crate::repl::live::modal::PendingModal>,
    rx: oneshot::Receiver<InteractionResponse>,
}

impl PermDriver {
    fn new(prompt: InteractionPrompt) -> Self {
        let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
        let (tx, rx) = oneshot::channel();
        let mut pending = None;
        super::super::modal::install_interaction(
            &mut controller,
            UiEvent::Interaction {
                prompt,
                responder: InteractionResponder { responder: tx },
            },
            &mut pending,
        );
        controller.redraw().unwrap();
        Self {
            controller,
            pending,
            rx,
        }
    }

    fn send(&mut self, code: KeyCode) {
        let _ = super::super::modal::handle_modal_key(
            &mut self.controller,
            KeyEvent::new(code, KeyModifiers::NONE),
            &mut self.pending,
        )
        .unwrap();
    }
}

fn sample_multiline_prompt() -> InteractionPrompt {
    let cmd = (1..=15)
        .map(|i| format!("echo line_{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    build_permission_prompt("bash", &json!({ "command": cmd }), &[])
}

#[test]
fn test_permission_prompt_layout_and_allow_flow() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    assert_eq!(
        driver.controller.state().active_modal().unwrap().option_layout,
        OptionLayout::Horizontal
    );

    driver.send(KeyCode::Enter);
    assert!(driver.controller.state().active_modal().is_none());
    assert_eq!(driver.rx.try_recv().unwrap(), InteractionResponse::Selected(0));
}

fn assert_edit_response(res: InteractionResponse) {
    match res {
        InteractionResponse::SelectedWithInput { index, text } => {
            assert_eq!(index, 1);
            assert!(text.ends_with('!'));
        }
        other => panic!("expected SelectedWithInput, got {other:?}"),
    }
}

#[test]
fn test_permission_prompt_edit_flow_with_multiline_input() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);
    assert!(
        driver
            .controller
            .state()
            .active_modal()
            .unwrap()
            .input
            .text()
            .contains("echo line_1")
    );

    driver.send(KeyCode::Char('!'));
    driver.send(KeyCode::Enter);
    assert!(driver.controller.state().active_modal().is_none());
    assert_edit_response(driver.rx.try_recv().unwrap());
}

fn assert_always_response(res: InteractionResponse) {
    match res {
        InteractionResponse::SelectedWithInput { index, text } => {
            assert_eq!(index, 2);
            assert!(text.contains("echo line_1"));
        }
        other => panic!("expected SelectedWithInput, got {other:?}"),
    }
}

fn assert_always_modal_input(driver: &PermDriver) {
    let modal = driver.controller.state().active_modal().unwrap();
    assert_eq!(
        modal.mode,
        crate::ui::interactive::ModalMode::Input {
            prompt_label: "pattern".into()
        }
    );
    assert!(modal.input.text().contains("echo line_1"));
}

#[test]
fn test_permission_prompt_always_flow() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Right);
    assert_eq!(driver.controller.state().active_modal().unwrap().selected, 2);

    driver.send(KeyCode::Enter);
    assert_always_modal_input(&driver);

    driver.send(KeyCode::Enter);
    assert!(driver.controller.state().active_modal().is_none());
    assert_always_response(driver.rx.try_recv().unwrap());
}

#[test]
fn test_permission_prompt_always_flow_escape_returns_to_select() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);
    assert!(driver.controller.state().active_modal().unwrap().mode != crate::ui::interactive::ModalMode::Select);

    driver.send(KeyCode::Esc);
    assert_eq!(
        driver.controller.state().active_modal().unwrap().mode,
        crate::ui::interactive::ModalMode::Select
    );
    assert_eq!(driver.controller.state().active_modal().unwrap().selected, 2);
    assert!(driver.rx.try_recv().is_err());
}

#[test]
fn test_permission_prompt_deny_flow_with_reason() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    for _ in 0..3 {
        driver.send(KeyCode::Char('l'));
    }
    driver.send(KeyCode::Enter);
    driver.send(KeyCode::Char('n'));
    driver.send(KeyCode::Char('o'));
    driver.send(KeyCode::Enter);
    assert_eq!(
        driver.rx.try_recv().unwrap(),
        InteractionResponse::SelectedWithInput {
            index: 3,
            text: "no".into()
        }
    );
}

#[test]
fn test_permission_prompt_scroll_multiline_body() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    driver.send(KeyCode::Down);
    assert_eq!(driver.controller.state().active_modal().unwrap().body_scroll, 1);
    driver.send(KeyCode::Char('j'));
    assert_eq!(driver.controller.state().active_modal().unwrap().body_scroll, 2);
    driver.send(KeyCode::Up);
    assert_eq!(driver.controller.state().active_modal().unwrap().body_scroll, 1);
    driver.send(KeyCode::Char('k'));
    assert_eq!(driver.controller.state().active_modal().unwrap().body_scroll, 0);
}

#[test]
fn test_permission_prompt_compound_seams_prefills_and_renders_formatted() {
    let prompt = build_permission_prompt("bash", &json!({ "command": "git status && cargo test ; ls" }), &[]);
    let mut driver = PermDriver::new(prompt);
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);
    let prefill = driver
        .controller
        .state()
        .active_modal()
        .unwrap()
        .input
        .text()
        .to_string();
    assert_eq!(prefill, "git status &&\n  cargo test;\nls");
}

#[test]
fn test_permission_prompt_input_mode_vertical_arrow_navigation() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);

    driver
        .controller
        .state_mut()
        .active_modal_mut()
        .unwrap()
        .input
        .move_to_start();
    let pos1 = driver.controller.state().active_modal().unwrap().input.cursor();
    assert_eq!(pos1, 0);

    driver.send(KeyCode::Down);
    let pos2 = driver.controller.state().active_modal().unwrap().input.cursor();
    assert!(pos2 > pos1);

    driver.send(KeyCode::Down);
    let pos3 = driver.controller.state().active_modal().unwrap().input.cursor();
    assert!(pos3 > pos2);

    driver.send(KeyCode::Up);
    let pos_back = driver.controller.state().active_modal().unwrap().input.cursor();
    assert_eq!(pos_back, pos2);
}

#[test]
fn test_permission_prompt_banner_bar_transitions() {
    let mut driver = PermDriver::new(sample_multiline_prompt());
    assert!(
        driver
            .controller
            .rendered()
            .unwrap()
            .top_divider
            .contains("Permission Required")
    );

    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);
    assert!(driver.controller.rendered().unwrap().top_divider.contains("edit"));

    driver.send(KeyCode::Esc);
    assert!(
        driver
            .controller
            .rendered()
            .unwrap()
            .top_divider
            .contains("Permission Required")
    );

    driver.send(KeyCode::Right);
    driver.send(KeyCode::Enter);
    assert!(driver.controller.rendered().unwrap().top_divider.contains("pattern"));
}
