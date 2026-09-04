use crate::ui::interactive::layout::render_running_tool_widget;
use crate::ui::interactive::layout::widget::RunningToolWidgetInput;
use crate::ui::interactive::state::RunningTool;
use crate::ui::theme::Theme;

#[test]
fn running_tool_widget_large_output_pre_slicing_and_skipped_count() {
    let theme = Theme::default();
    let mut tool = RunningTool::new("bash", "seq 1 120", None);
    let mut output = String::new();
    for i in 1..=120 {
        output.push_str(&format!("line {i}\n"));
    }
    tool.append_chunk(&output);

    // Collapsed view: pre-slicing activates (120 > 50).
    // Total lines = 120, shown visual lines = 5 (lines 116..120), skipped = 115.
    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(
        full.contains("... (115 earlier lines)"),
        "expected 115 skipped lines, got:\n{full}"
    );
    assert!(full.contains("line 116"));
    assert!(full.contains("line 120"));
    assert!(
        !full.contains("line 1\n") && !full.contains("line 50\n") && !full.contains("line 100\n"),
        "earlier lines should not be rendered in collapsed view"
    );

    // Expanded view: renders all lines
    let lines_expanded = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: true,
    });
    let full_expanded = lines_expanded.join("\n");
    assert!(full_expanded.contains("line 1"));
    assert!(full_expanded.contains("line 120"));
    assert!(!full_expanded.contains("earlier lines"));
}

#[test]
fn running_tool_widget_pre_slice_boundary_50_and_51_lines() {
    let theme = Theme::default();

    // Exactly 50 lines: does not pre-slice (boundary is > 50).
    let mut tool_50 = RunningTool::new("bash", "seq 1 50", None);
    let mut out_50 = String::new();
    for i in 1..=50 {
        out_50.push_str(&format!("line {i}\n"));
    }
    tool_50.append_chunk(&out_50);

    let lines_50 = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_50,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full_50 = lines_50.join("\n");
    assert!(
        full_50.contains("... (45 earlier lines)"),
        "50 lines should show 45 earlier lines (50 - 5)"
    );
    assert!(full_50.contains("line 50"));

    // 51 lines: triggers pre-slicing (51 > 50).
    let mut tool_51 = RunningTool::new("bash", "seq 1 51", None);
    let mut out_51 = String::new();
    for i in 1..=51 {
        out_51.push_str(&format!("line {i}\n"));
    }
    tool_51.append_chunk(&out_51);

    let lines_51 = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_51,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full_51 = lines_51.join("\n");
    assert!(
        full_51.contains("... (46 earlier lines)"),
        "51 lines should show 46 earlier lines (51 - 5)"
    );
    assert!(full_51.contains("line 47"));
    assert!(full_51.contains("line 51"));
    assert!(!full_51.contains("line 46\n"));
}

#[test]
fn running_tool_widget_large_output_with_soft_wrapping() {
    let theme = Theme::default();
    let mut tool = RunningTool::new("bash", "wrapped", None);
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
