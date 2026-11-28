use crate::ui::interactive::NullBackend;
use crate::ui::interactive::{
    InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse, ModalMode, ModalState,
    OptionLayout, TerminalController, UiEvent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use test_case::test_case;
use tokio::sync::oneshot;

struct NavFixture {
    controller: TerminalController<NullBackend>,
    pending: Option<crate::repl::live::modal::PendingModal>,
    _rx: oneshot::Receiver<InteractionResponse>,
}

impl NavFixture {
    fn new(layout: OptionLayout) -> Self {
        let mut controller = crate::repl::test_controller();
        let (tx, _rx) = oneshot::channel();
        let mut pending = None;
        super::super::modal::install_interaction(
            &mut controller,
            UiEvent::Interaction {
                prompt: sample_prompt(layout),
                responder: InteractionResponder { responder: tx },
            },
            &mut pending,
        )
        .unwrap();
        Self {
            controller,
            pending,
            _rx,
        }
    }

    fn send(&mut self, code: KeyCode) {
        self.send_key(key(code));
    }

    fn send_key(&mut self, key: KeyEvent) {
        let _ = super::super::modal::handle_modal_key(&mut self.controller, key, &mut self.pending).unwrap();
    }
}

type Step = (KeyEvent, usize);

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn assert_steps(layout: OptionLayout, steps: &[Step], observed: fn(&ModalState) -> usize) {
    let mut fix = NavFixture::new(layout);
    for (step_key, expected) in steps {
        fix.send_key(*step_key);
        let modal = fix.controller.state().active_modal().unwrap();
        assert_eq!(observed(modal), *expected);
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

#[test_case(&[(key(KeyCode::Right), 1), (key(KeyCode::Char('l')), 2)]; "right then l")]
#[test_case(
    &[
        (key(KeyCode::Right), 1),
        (key(KeyCode::Right), 2),
        (key(KeyCode::Left), 1),
        (key(KeyCode::Char('h')), 0),
    ];
    "left then h"
)]
fn horizontal_mode_arrow_and_vi_keys_move_selection(steps: &[Step]) {
    assert_steps(OptionLayout::Horizontal, steps, |m| m.selected);
}

#[test_case(&[(key(KeyCode::Down), 1), (key(KeyCode::Char('j')), 2)]; "down then j")]
#[test_case(
    &[
        (key(KeyCode::Down), 1),
        (key(KeyCode::Down), 2),
        (key(KeyCode::Up), 1),
        (key(KeyCode::Char('k')), 0),
    ];
    "up then k"
)]
fn horizontal_mode_arrow_and_vi_keys_scroll_body(steps: &[Step]) {
    assert_steps(OptionLayout::Horizontal, steps, |m| m.body_scroll);
}

#[test]
fn test_scroll_down_caps_at_bottom_and_up_scrolls_immediately() {
    let mut fix = NavFixture::new(OptionLayout::Horizontal);
    for _ in 0..100 {
        fix.send(KeyCode::Down);
    }
    let bottom_scroll = fix.controller.state().active_modal().unwrap().body_scroll;
    assert!(bottom_scroll > 0);
    fix.send(KeyCode::Down);
    assert_eq!(
        fix.controller.state().active_modal().unwrap().body_scroll,
        bottom_scroll
    );
    fix.send(KeyCode::Up);
    assert_eq!(
        fix.controller.state().active_modal().unwrap().body_scroll,
        bottom_scroll - 1
    );
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

#[test_case(
    &[(key(KeyCode::Tab), 1), (KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT), 0)];
    "shift tab"
)]
#[test_case(&[(key(KeyCode::Tab), 1), (key(KeyCode::BackTab), 0)]; "backtab")]
fn modal_tab_keys_move_selection_backward(steps: &[Step]) {
    assert_steps(OptionLayout::Vertical, steps, |m| m.selected);
}
