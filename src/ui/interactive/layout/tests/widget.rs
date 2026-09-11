use crate::ui::interactive::layout::widget::RunningToolWidgetInput;
use crate::ui::interactive::layout::{LayoutInput, layout, render_running_tool_widget};
use crate::ui::interactive::state::RunningTool;
use crate::ui::interactive::{EditorState, FooterState};
use crate::ui::theme::Theme;

#[test]
fn widget_lines_affect_height_and_cursor_row() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let widgets = vec![
        "● Todos (1/2)".to_string(),
        "├─ ✓ #1 Done".to_string(),
        "└─ ○ #2 Pending".to_string(),
    ];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    });

    assert_eq!(layout.widget_lines.len(), 3);
    assert_eq!(layout.height(), 10);
    assert_eq!(layout.cursor_row(), 6);
}

fn render_test_widget(tool: &RunningTool, expanded: bool) -> String {
    render_running_tool_widget(RunningToolWidgetInput {
        tool,
        theme: &Theme::default(),
        width: 60,
        tools_expanded: expanded,
    })
    .join("\n")
}

#[test]
fn running_tool_widget_collapsed_header_and_tail() {
    let mut tool = RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");
    let full = render_test_widget(&tool, false);

    assert!(full.contains("bash") && full.contains("cargo test"));
    assert!(full.contains("... (2 earlier lines)") && full.contains("line 7"));
    assert!(!full.contains("line 1\n") && full.contains("Elapsed"));
}

#[test]
fn running_tool_widget_expanded_shows_all() {
    let mut tool = RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");
    let full = render_test_widget(&tool, true);

    assert!(full.contains("line 1") && full.contains("line 7"));
    assert!(!full.contains("earlier lines"));
}

#[test]
fn running_tool_widget_with_preview_renders_diff_card() {
    let preview = Some("+ line added\n- line removed".to_string());
    let tool = RunningTool::new("edit", "src/main.rs", preview);
    let full = render_test_widget(&tool, false);
    for token in ["edit", "src/main.rs", "+ line added", "- line removed", "Elapsed"] {
        assert!(full.contains(token));
    }
}

#[test]
fn running_tool_widget_empty_for_fast_tools_without_preview_or_output() {
    let theme = Theme::default();
    let tool_fd = RunningTool::new("fd", "pattern in .", None);
    let lines_fd = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_fd,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_fd.is_empty(), "fd should not render a running widget card");

    let tool_rg = RunningTool::new("rg", "/pattern/ in .", None);
    let lines_rg = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_rg,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_rg.is_empty(), "rg should not render a running widget card");
}
