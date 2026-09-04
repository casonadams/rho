use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn default_keybindings_include_model_select_and_cycle() {
    let map = default_keybindings();
    let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert_eq!(map.get_action(&ctrl_l), Some(KeyAction::AppModelSelect));

    let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
    assert_eq!(map.get_action(&ctrl_p), Some(KeyAction::AppModelCycleForward));

    let shift_ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SHIFT | KeyModifiers::CONTROL);
    assert_eq!(map.get_action(&shift_ctrl_p), Some(KeyAction::AppModelCycleBackward));
}

#[test]
fn custom_toml_overrides_keybindings() {
    let temp = tempfile::tempdir().unwrap();
    let toml_file = temp.path().join("keybindings.toml");
    std::fs::write(
        &toml_file,
        r#"
"app.model.select" = "ctrl+m"
"tui.editor.deleteWordBackward" = ["ctrl+w", "alt+backspace"]
"app.thinking.cycle" = []
"#,
    )
    .unwrap();

    let map = load_keybindings(temp.path());
    let ctrl_m = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL);
    assert_eq!(map.get_action(&ctrl_m), Some(KeyAction::AppModelSelect));

    let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert_eq!(map.get_action(&ctrl_l), None);

    let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
    assert_eq!(map.get_action(&shift_tab), None);
}
