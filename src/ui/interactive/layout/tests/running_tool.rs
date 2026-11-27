use crate::ui::interactive::layout::render_running_tool_widget;
use crate::ui::interactive::layout::widget::RunningToolWidgetInput;
use crate::ui::interactive::state::RunningTool;
use crate::ui::theme::Theme;

fn tool_with_n_lines(n: usize) -> RunningTool {
    let command = format!("seq 1 {n}");
    let mut tool = RunningTool::new("bash", &command, serde_json::json!({"command": command}));
    let out = (1..=n).map(|i| format!("line {i}\n")).collect::<String>();
    tool.append_chunk(&out);
    tool
}

fn render_tool_widget(tool: &RunningTool, expanded: bool) -> String {
    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool,
        theme: &Theme::default(),
        width: 60,
        tools_expanded: expanded,
    });
    lines.join("\n")
}

#[test]
fn running_tool_widget_large_output_collapsed() {
    let tool = tool_with_n_lines(120);
    let full = render_tool_widget(&tool, false);

    assert!(full.contains("... (115 earlier lines, Ctrl+O to expand)"));
    assert!(full.contains("line 116") && full.contains("line 120"));
    assert!(!full.contains("line 1\n") && !full.contains("line 50\n") && !full.contains("line 100\n"));
}

#[test]
fn running_tool_widget_large_output_expanded() {
    let tool = tool_with_n_lines(120);
    let full_expanded = render_tool_widget(&tool, true);

    assert!(full_expanded.contains("line 1") && full_expanded.contains("line 120"));
    assert!(!full_expanded.contains("earlier lines"));
}

#[test]
fn running_tool_widget_boundary_50_lines() {
    let tool_50 = tool_with_n_lines(50);
    let full_50 = render_tool_widget(&tool_50, false);
    assert!(full_50.contains("... (45 earlier lines, Ctrl+O to expand)") && full_50.contains("line 50"));
}

#[test]
fn running_tool_widget_boundary_51_lines() {
    let tool_51 = tool_with_n_lines(51);
    let full_51 = render_tool_widget(&tool_51, false);
    assert!(full_51.contains("... (46 earlier lines, Ctrl+O to expand)"));
    assert!(full_51.contains("line 47") && full_51.contains("line 51") && !full_51.contains("line 46\n"));
}

#[test]
fn running_tool_widget_large_output_with_soft_wrapping() {
    let theme = Theme::default();
    let mut tool = RunningTool::new("bash", "wrapped", serde_json::json!({"command": "wrapped"}));
    let mut output = String::new();
    for i in 1..=55 {
        output.push_str(&format!("line {i}\n"));
    }
    // Add a wide line at line 56 that wraps into 2 visual lines at width 30
    output
        .push_str("line 56: this is a very long line that will definitely wrap across multiple visual terminal rows\n");
    tool.append_chunk(&output);

    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    // 56 total logical lines. Line 56 wraps into 4 visual lines at inner width 26.
    // 55 single lines + 4 wrapped lines = 59 visual lines. Showing 5 visual lines means 54 skipped.
    assert!(full.contains("earlier lines"));
    assert!(full.contains("line 56"));
}

fn streamed_tool(name: &str) -> RunningTool {
    let mut tool = RunningTool::new(name, "step", serde_json::json!({"prompt": "step"}));
    tool.append_chunk(&(1..=8).map(|i| format!("step line {i}\n")).collect::<String>());
    tool
}

#[test]
fn running_subagent_and_workflow_cards_show_streamed_output_tail() {
    let structured = Theme {
        structured_tool_cards: true,
        ..Theme::default()
    };
    for name in ["subagent", "workflow"] {
        for theme in [Theme::default(), structured.clone()] {
            let tool = streamed_tool(name);
            let lines = render_running_tool_widget(RunningToolWidgetInput {
                tool: &tool,
                theme: &theme,
                width: 60,
                tools_expanded: false,
            });
            let text = crate::ui::block::strip_ansi(&lines.join("\n"));
            assert!(
                text.contains("... (3 earlier lines, Ctrl+O to expand)"),
                "{name}: {text}"
            );
            assert!(
                text.contains("step line 4") && text.contains("step line 8"),
                "{name}: {text}"
            );
            assert!(!text.contains("step line 3"), "{name}: {text}");
        }
    }
}

#[test]
fn running_tool_widget_starts_with_empty_line_pad() {
    let mut theme = Theme::default();
    theme.block_style = crate::ui::theme::BlockStyle::Solid;
    let tool = RunningTool::new("bash", "echo hello", serde_json::json!({"command": "echo hello"}));
    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(!lines.is_empty());
    assert_eq!(
        lines[0], "",
        "running tool widget must start with an empty line to preserve padding between blocks"
    );
}
