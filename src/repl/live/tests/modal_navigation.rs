use super::common::HistoryTerminal;
use crate::ui::interactive::{
    InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse, InteractiveState, ModalMode,
    OptionLayout, TerminalController, UiEvent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::oneshot;

struct NavFixture {
    controller: TerminalController<HistoryTerminal>,
    pending: Option<crate::repl::live::modal::PendingModal>,
    _rx: oneshot::Receiver<InteractionResponse>,
}

impl NavFixture {
    fn new(layout: OptionLayout) -> Self {
        let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
        let (tx, _rx) = oneshot::channel();
        let mut pending = None;
        super::super::modal::install_interaction(
            &mut controller,
            UiEvent::Interaction {
                prompt: sample_prompt(layout),
                responder: InteractionResponder { responder: tx },
            },
            &mut pending,
        );
        Self {
            controller,
            pending,
            _rx,
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

fn sample_prompt(layout: OptionLayout) -> InteractionPrompt {
    let body = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let options = ["Allow", "Edit", "Always", "Deny"]
        .into_iter()
        .map(|l| InteractionOption {
            label: l.into(),
            description: None,
            input: None,
        })
        .collect();
    InteractionPrompt {
        title: "Permission".into(),
        body,
        options,
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: layout,
    }
}

#[test]
fn test_horizontal_mode_right_and_l_navigate_forward() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    fix.send(KeyCode::Right);
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 1);
    fix.send(KeyCode::Char('l'));
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 2);
}

#[test]
fn test_horizontal_mode_left_and_h_navigate_backward() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    fix.send(KeyCode::Right);
    fix.send(KeyCode::Right);
    fix.send(KeyCode::Left);
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 1);
    fix.send(KeyCode::Char('h'));
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 0);
}

#[test]
fn test_horizontal_mode_down_and_j_scroll_body() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    fix.send(KeyCode::Down);
    assert_eq!(fix.controller.state().active_modal().unwrap().body_scroll, 1);
    fix.send(KeyCode::Char('j'));
    assert_eq!(fix.controller.state().active_modal().unwrap().body_scroll, 2);
}

#[test]
fn test_horizontal_mode_up_and_k_scroll_body() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    fix.send(KeyCode::Down);
    fix.send(KeyCode::Down);
    fix.send(KeyCode::Up);
    assert_eq!(fix.controller.state().active_modal().unwrap().body_scroll, 1);
    fix.send(KeyCode::Char('k'));
    assert_eq!(fix.controller.state().active_modal().unwrap().body_scroll, 0);
}

#[test]
fn test_vertical_mode_jk_navigates_options_without_horizontal_scroll() {
    let mut fix = NavFixture::new(OptionLayout::Vertical);
    fix.send(KeyCode::Down);
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 1);
    assert_eq!(fix.controller.state().active_modal().unwrap().body_scroll, 0);
    fix.send(KeyCode::Char('j'));
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 2);
    fix.send(KeyCode::Char('k'));
    assert_eq!(fix.controller.state().active_modal().unwrap().selected, 1);
}

#[test]
fn test_input_mode_isolation_types_characters_without_scrolling() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    fix.controller
        .state_mut()
        .active_modal_mut()
        .unwrap()
        .enter_input_mode("reason");
    for ch in ['h', 'j', 'k', 'l'] {
        fix.send(KeyCode::Char(ch));
    }
    let modal = fix.controller.state().active_modal().unwrap();
    assert_eq!((modal.input.text(), modal.body_scroll, modal.selected), ("hjkl", 0, 0));
    fix.send(KeyCode::Esc);
    assert!(matches!(
        fix.controller.state().active_modal().unwrap().mode,
        ModalMode::Select
    ));
}
