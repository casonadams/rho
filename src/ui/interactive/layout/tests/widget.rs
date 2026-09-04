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
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.widget_lines.len(), 3);
    // 3 (widgets) + 1 (spacer) + 1 (top_divider) + 1 (editor) + 1 (bottom_divider) + 2 (footer) = 9
    assert_eq!(layout.height(), 9);
    // cursor_row: 3 (widgets) + 1 (spacer) + 1 (top_divider) + 0 (editor cursor row) = 5
    assert_eq!(layout.cursor_row(), 5);
}

#[test]
fn running_tool_widget_renders_header_tail_and_elapsed() {
    let theme = Theme::default();
    let mut tool = RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");

    // Collapsed view (tools_expanded = false)
    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(full.contains("bash"), "should contain tool name");
    assert!(full.contains("cargo test"), "should contain command");
    assert!(
        full.contains("... (2 earlier lines)"),
        "should show skipped lines count"
    );
    assert!(full.contains("line 7"), "should show latest tailed lines");
    assert!(
        !full.contains("line 1\n"),
        "earlier line 1 should be truncated from tail preview"
    );
    assert!(full.contains("Elapsed"), "should contain elapsed duration");

    // Expanded view (tools_expanded = true)
    let lines_expanded = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: true,
    });
    let full_expanded = lines_expanded.join("\n");
    assert!(
        full_expanded.contains("line 1"),
        "expanded view should show earlier lines"
    );
    assert!(full_expanded.contains("line 7"));
    assert!(
        !full_expanded.contains("earlier lines"),
        "expanded view should not have skip hint"
    );
}

#[test]
fn running_tool_widget_with_preview_renders_diff_card() {
    let theme = Theme::default();
    let preview = Some("+ line added\n- line removed".to_string());
    let tool = RunningTool::new("edit", "src/main.rs", preview);

    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(full.contains("edit"));
    assert!(full.contains("src/main.rs"));
    assert!(full.contains("+ line added"));
    assert!(full.contains("- line removed"));
    assert!(full.contains("Elapsed"));
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
