use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use rho::ui::editor::{EditorMode, TextAreaEditor, VimMode};
use rho::ui::{PromptEditor, TerminalComponent};
use rho_harness_core::config::Config;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

#[test]
fn test_default_mode_typing() {
    let mut editor = TextAreaEditor::new(EditorMode::Default);
    assert!(editor.is_empty());
    assert_eq!(editor.mode_label(), "");

    assert!(editor.handle_key(key(KeyCode::Char('h'))));
    assert!(editor.handle_key(key(KeyCode::Char('i'))));
    assert_eq!(editor.text(), "hi");
    assert!(!editor.is_empty());

    // Arrow keys move cursor without submitting (return true)
    assert!(editor.handle_key(key(KeyCode::Left)));
    assert_eq!(editor.cursor(), (0, 1));
    assert!(editor.handle_key(key(KeyCode::Right)));
    assert_eq!(editor.cursor(), (0, 2));

    // Enter without modifiers returns false to signal submission
    assert!(!editor.handle_key(key(KeyCode::Enter)));
    assert_eq!(editor.text(), "hi");

    editor.clear();
    assert!(editor.is_empty());
    assert_eq!(editor.text(), "");
}

#[test]
fn test_default_mode_newlines_and_backspace() {
    let mut editor = TextAreaEditor::new(EditorMode::Default);
    editor.set_text("hi");

    // Shift+Enter inserts newline
    assert!(editor.handle_key(shift(KeyCode::Enter)));
    assert!(editor.handle_key(key(KeyCode::Char('!'))));
    assert_eq!(editor.text(), "hi\n!");

    assert!(editor.handle_key(key(KeyCode::Backspace)));
    assert_eq!(editor.text(), "hi\n");
}

#[test]
fn test_vim_mode_insert_transition() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    assert_eq!(editor.vim_mode(), VimMode::Normal);
    assert_eq!(editor.mode_label(), "NORMAL");

    // 'i' -> Insert
    assert!(editor.handle_key(key(KeyCode::Char('i'))));
    assert_eq!(editor.vim_mode(), VimMode::Insert);
    assert_eq!(editor.mode_label(), "INSERT");

    assert!(editor.handle_key(key(KeyCode::Char('a'))));
    assert!(editor.handle_key(key(KeyCode::Char('b'))));
    assert_eq!(editor.text(), "ab");

    // Esc -> Normal
    assert!(editor.handle_key(key(KeyCode::Esc)));
    assert_eq!(editor.vim_mode(), VimMode::Normal);
    assert_eq!(editor.mode_label(), "NORMAL");
}

#[test]
fn test_vim_mode_visual_transition() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);

    // 'v' -> Visual
    assert!(editor.handle_key(key(KeyCode::Char('v'))));
    assert_eq!(editor.vim_mode(), VimMode::Visual);
    assert_eq!(editor.mode_label(), "VISUAL");

    assert!(editor.handle_key(key(KeyCode::Esc)));
    assert_eq!(editor.vim_mode(), VimMode::Normal);

    // 'V' -> VisualLine
    assert!(editor.handle_key(shift(KeyCode::Char('V'))));
    assert_eq!(editor.vim_mode(), VimMode::VisualLine);
    assert_eq!(editor.mode_label(), "VISUAL LINE");

    assert!(editor.handle_key(key(KeyCode::Esc)));
    assert_eq!(editor.vim_mode(), VimMode::Normal);
}

#[test]
fn test_vim_mode_replace_transition() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("cat");
    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert!(editor.handle_key(key(KeyCode::Char('g'))));

    // 'r' -> Replace once
    assert!(editor.handle_key(key(KeyCode::Char('r'))));
    assert_eq!(editor.vim_mode(), VimMode::Replace(true));
    assert_eq!(editor.mode_label(), "REPLACE ONCE");

    assert!(editor.handle_key(key(KeyCode::Char('b'))));
    assert_eq!(editor.vim_mode(), VimMode::Normal);
    assert_eq!(editor.text(), "bat");

    // 'R' -> Replace mode (overtype)
    assert!(editor.handle_key(shift(KeyCode::Char('R'))));
    assert_eq!(editor.vim_mode(), VimMode::Replace(false));
    assert_eq!(editor.mode_label(), "REPLACE");

    assert!(editor.handle_key(key(KeyCode::Esc)));
    assert_eq!(editor.vim_mode(), VimMode::Normal);
}

#[test]
fn test_vim_motions_cursor() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("first second third\nfourth fifth");

    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert_eq!(editor.cursor(), (0, 0));

    assert!(editor.handle_key(key(KeyCode::Char('w'))));
    assert!(editor.handle_key(key(KeyCode::Char('e'))));
    assert!(editor.handle_key(key(KeyCode::Char('b'))));
    assert!(editor.handle_key(key(KeyCode::Char('0'))));
    assert_eq!(editor.cursor(), (0, 0));

    assert!(editor.handle_key(key(KeyCode::Char('$'))));
    assert_eq!(editor.cursor(), (0, 18));
}

#[test]
fn test_vim_motions_jumps_and_undo() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("first second third\nfourth fifth");

    assert!(editor.handle_key(shift(KeyCode::Char('G'))));
    assert_eq!(editor.cursor(), (1, 0));

    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert_eq!(editor.cursor(), (0, 0));

    // 'x' deletes char under cursor
    assert!(editor.handle_key(key(KeyCode::Char('x'))));
    assert_eq!(editor.text(), "irst second third\nfourth fifth");

    // 'u' undoes
    assert!(editor.handle_key(key(KeyCode::Char('u'))));
    assert_eq!(editor.text(), "first second third\nfourth fifth");

    // Ctrl+r redoes
    assert!(editor.handle_key(ctrl('r')));
    assert_eq!(editor.text(), "irst second third\nfourth fifth");
}

#[test]
fn test_vim_operators_deletion() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("line 1\nline 2");

    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert!(editor.handle_key(key(KeyCode::Char('g'))));
    assert!(editor.handle_key(key(KeyCode::Char('d'))));
    assert!(editor.handle_key(key(KeyCode::Char('d'))));
    assert_eq!(editor.text(), "line 2");
}

#[test]
fn test_vim_visual_mode_yank_and_cut() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("hello world");

    assert!(editor.handle_key(key(KeyCode::Char('0'))));
    assert_eq!(editor.cursor(), (0, 0));

    // 'v' visual mode, move right 5 times
    assert!(editor.handle_key(key(KeyCode::Char('v'))));
    for _ in 0..5 {
        assert!(editor.handle_key(key(KeyCode::Char('l'))));
    }

    // 'd' to delete selection
    assert!(editor.handle_key(key(KeyCode::Char('d'))));
    assert_eq!(editor.vim_mode(), VimMode::Normal);
    assert_eq!(editor.text(), "world");

    // 'p' to paste
    assert!(editor.handle_key(key(KeyCode::Char('p'))));
    assert!(editor.text().contains("hello") || editor.text().contains("world"));
}

#[test]
fn test_collapsed_paste_markers() {
    let mut editor = TextAreaEditor::new(EditorMode::Default);

    editor.handle_paste("small paste content");
    assert_eq!(editor.text(), "small paste content");
    assert_eq!(editor.pastes().len(), 0);

    editor.clear();
    let large_paste = (1..=15).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    editor.handle_paste(&large_paste);
    assert!(editor.text().starts_with("[paste #1 +15 lines]"));
    assert_eq!(editor.pastes().len(), 1);

    let expanded = editor.expanded_text();
    assert_eq!(expanded, large_paste);

    let queued = editor.take_submission(rho::ui::interactive::QueueKind::Steering);
    assert!(queued.is_some());
    assert_eq!(queued.unwrap().text, large_paste);
    assert!(editor.is_empty());
    assert_eq!(editor.pastes().len(), 0);
}

#[test]
fn test_clipboard_image_marker_insertion() {
    let mut editor = TextAreaEditor::new(EditorMode::Default);
    let temp_img = std::path::Path::new("/tmp/test-image.png");
    editor.handle_clipboard_image(temp_img);
    assert_eq!(editor.text(), "[image /tmp/test-image.png]");
}

#[test]
fn test_editor_config_option() {
    #[derive(serde::Deserialize)]
    struct ConfigWrapper {
        editor: rho_harness_core::config::EditorConfig,
    }

    let toml_vim = r#"
[editor]
mode = "vim"
"#;
    let mut config = Config::default();
    let wrapper: ConfigWrapper = toml::from_str(toml_vim).unwrap();
    config.editor.merge(&wrapper.editor);
    assert!(config.editor.is_vim());
    assert_eq!(config.editor.mode.as_deref(), Some("vim"));

    let toml_default = r#"
[editor]
mode = "default"
"#;
    let mut config2 = Config::default();
    let wrapper2: ConfigWrapper = toml::from_str(toml_default).unwrap();
    config2.editor.merge(&wrapper2.editor);
    assert!(!config2.editor.is_vim());
}

#[test]
fn test_terminal_component_rendering() {
    let mut editor = TextAreaEditor::new(EditorMode::Vim);
    editor.set_text("code to render");
    editor.set_placeholder("Type prompt here...");

    assert_eq!(editor.desired_height(80), 1);

    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            editor.render(f, Rect::new(0, 0, 80, 5));
        })
        .unwrap();

    let view = terminal.backend().to_string();
    assert!(view.contains("code to render"));
}
