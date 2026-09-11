use super::common::HistoryTerminal;
use crate::repl::ReplSession;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_thinking_selector};
use crate::ui::interactive::{EditorState, FooterState, InteractiveState, LayoutInput, TerminalController, layout};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::config::Config;

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn send_modal_key_with_mods(
    c: &mut TerminalController<HistoryTerminal>,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, modifiers), &mut None).unwrap()
}

fn setup_thinking_controller(level: Option<&str>) -> TerminalController<HistoryTerminal> {
    let config = Config {
        thinking_level: level.map(ToString::to_string),
        ..Default::default()
    };
    let session = ReplSession::new(config, crate::auth::AuthStore::default(), None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_thinking_selector(&session, &mut controller);
    controller
}

#[test]
fn thinking_selector_opens_with_all_levels() {
    let controller = setup_thinking_controller(Some("medium"));
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Select Thinking Level");
    assert_eq!(modal.options.len(), 7);
    assert_eq!(modal.selected, 3);
}

#[test]
fn thinking_selector_marks_active_level() {
    let controller = setup_thinking_controller(Some("medium"));
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options[3].description.as_deref().unwrap().contains('✓'));
    assert!(!modal.options[0].description.as_deref().unwrap().contains('✓'));
}

#[test]
fn thinking_selector_navigates_with_arrows_and_jk() {
    let mut controller = setup_thinking_controller(Some("off"));
    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);
    let _ = send_modal_key(&mut controller, KeyCode::Char('j'));
    assert_eq!(controller.state().active_modal().unwrap().selected, 2);
    let _ = send_modal_key(&mut controller, KeyCode::Up);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);
    let _ = send_modal_key(&mut controller, KeyCode::Char('k'));
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);
}

#[test]
fn thinking_selector_navigates_with_digits() {
    let mut controller = setup_thinking_controller(Some("off"));
    let _ = send_modal_key(&mut controller, KeyCode::Char('5'));
    assert_eq!(controller.state().active_modal().unwrap().selected, 4);
    let _ = send_modal_key(&mut controller, KeyCode::Char('1'));
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);
    let _ = send_modal_key(&mut controller, KeyCode::Char('7'));
    assert_eq!(controller.state().active_modal().unwrap().selected, 6);
}

#[test]
fn thinking_selector_selects_on_enter() {
    let mut controller = setup_thinking_controller(Some("off"));
    let _ = send_modal_key(&mut controller, KeyCode::Char('5'));
    let res = send_modal_key(&mut controller, KeyCode::Enter);

    assert_eq!(
        res,
        ModalKeyResult::ThinkingLevelSelected {
            level: Some("high".to_string()),
            save_as_default: false,
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn thinking_selector_select_off_returns_none_level() {
    let mut controller = setup_thinking_controller(Some("high"));
    let _ = send_modal_key(&mut controller, KeyCode::Char('1'));
    let res = send_modal_key(&mut controller, KeyCode::Enter);

    assert_eq!(
        res,
        ModalKeyResult::ThinkingLevelSelected {
            level: None,
            save_as_default: false,
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn thinking_selector_ctrl_s_saves_as_default() {
    let mut controller = setup_thinking_controller(Some("off"));
    let _ = send_modal_key(&mut controller, KeyCode::Char('4'));
    let res = send_modal_key_with_mods(&mut controller, KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert_eq!(
        res,
        ModalKeyResult::ThinkingLevelSelected {
            level: Some("medium".to_string()),
            save_as_default: true,
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn thinking_selector_cancels_on_esc() {
    let mut controller = setup_thinking_controller(Some("off"));
    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn thinking_selector_cancels_on_ctrl_c() {
    let mut controller = setup_thinking_controller(Some("off"));
    let res = send_modal_key_with_mods(&mut controller, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn thinking_selector_layout_rendering_with_theme() {
    let controller = setup_thinking_controller(Some("medium"));
    let modal = controller.state().active_modal().unwrap();

    let rendered = layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: Some(&crate::ui::theme::Theme::default()),
        focused: true,
    });

    let full_text = rendered.lines.join("\n");
    assert!(full_text.contains("Select Thinking Level"));
    assert!(full_text.contains("medium"));
    assert!(full_text.contains("Enter to select"));
}
