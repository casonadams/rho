use super::base_input;
use crate::ui::interactive::layout::widget::RunningToolWidgetInput;
use crate::ui::interactive::layout::{LayoutInput, layout, render_running_tool_widget};
use crate::ui::interactive::state::RunningTool;
use crate::ui::interactive::{EditorState, FooterState};
use crate::ui::theme::Theme;
use serde_json::json;

#[test]
fn widget_lines_affect_height_and_cursor_row() {
    let widgets = vec![
        "● Todos (1/2)".to_string(),
        "├─ ✓ #1 Done".to_string(),
        "└─ ○ #2 Pending".to_string(),
    ];
    let layout = layout(LayoutInput {
        widget_lines: &widgets,
        ..base_input(&EditorState::default(), &FooterState::default())
    });

    assert_eq!(layout.widget_lines.len(), 3);
    assert_eq!(layout.height(), 9);
    assert_eq!(layout.cursor_row(), 5);
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
    let mut tool = RunningTool::new("bash", "cargo test", json!({"command": "cargo test"}));
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");
    let full = crate::ui::block::strip_ansi(&render_test_widget(&tool, false));

    assert!(full.contains("bash") && full.contains("cargo test"));
    assert!(full.contains("... (2 earlier lines, Ctrl+O to expand)") && full.contains("line 7"));
    assert!(!full.contains("line 1\n") && full.contains("Elapsed "));
}

#[test]
fn running_tool_widget_expanded_shows_all() {
    let mut tool = RunningTool::new("bash", "cargo test", json!({"command": "cargo test"}));
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");
    let full = render_test_widget(&tool, true);

    assert!(full.contains("line 1") && full.contains("line 7"));
    assert!(!full.contains("earlier lines"));
}

#[test]
fn running_tool_widget_bash_with_timeout_styles_timeout_with_dimmed() {
    let theme = Theme::default();
    let mut tool = RunningTool::new(
        "bash",
        "cargo test (timeout 300s)",
        json!({"command": "cargo test", "timeout": 300}),
    );
    tool.append_chunk("running...\n");
    let lines = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 80,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    let dim = theme.dimmed;
    assert!(full.contains(&format!("{dim}(timeout 300s){dim:#}")));
    assert!(full.contains(&format!("{dim}Elapsed")));
}

#[test]
fn running_tool_widget_renders_subagent_before_any_output() {
    let tool = RunningTool::new("subagent", "prompt=\"find callers\"", json!({"prompt": "find callers"}));
    let full = render_test_widget(&tool, false);
    for token in ["subagent", "find callers", "Elapsed"] {
        assert!(full.contains(token), "missing {token}");
    }
}

#[test]
fn running_tool_widget_empty_for_fast_tools_without_preview_or_output() {
    let theme = Theme::default();
    let tool_fd = RunningTool::new("fd", "pattern in .", json!({"pattern": "pattern"}));
    let lines_fd = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_fd,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_fd.is_empty(), "fd should not render a running widget card");

    let tool_rg = RunningTool::new("rg", "/pattern/ in .", json!({"pattern": "pattern"}));
    let lines_rg = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool_rg,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_rg.is_empty(), "rg should not render a running widget card");
}

#[test]
fn layout_windows_oversized_bordered_widget_preserving_box_integrity() {
    let mut theme = Theme::default();
    theme.block_style = crate::ui::theme::BlockStyle::Border;
    let mut tool = RunningTool::new("bash", "git log -n 50", json!({"command": "git log -n 50"}));
    for i in 1..=40 {
        tool.append_chunk(&format!("commit line {i}\n"));
    }
    let widgets = render_running_tool_widget(RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: true,
    });
    assert!(widgets.len() > 15);
    assert!(widgets[0].contains('╭'));
    assert!(widgets.last().unwrap().contains('╰'));

    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let l = layout(LayoutInput {
        widget_lines: &widgets,
        terminal_width: 60,
        terminal_height: 20,
        theme: Some(&theme),
        ..base_input(&default_editor, &default_footer)
    });

    assert!(l.widget_lines.len() <= 15);
    assert!(
        l.widget_lines.first().unwrap().contains('╭'),
        "top border must be preserved"
    );
    let header_plain = crate::ui::block::strip_ansi(&l.widget_lines[1]);
    assert!(
        header_plain.contains("bash") && header_plain.contains("git log"),
        "header must be preserved"
    );
    assert!(
        l.widget_lines.last().unwrap().contains('╰'),
        "bottom border must be preserved"
    );
}

#[test]
fn below_widget_lines_render_between_bottom_divider_and_footer() {
    let below = vec!["[hint: press Tab]".to_string()];
    let ed = EditorState::default();
    let ft = FooterState::default();
    let l = layout(LayoutInput {
        below_widget_lines: &below,
        terminal_height: 24,
        ..base_input(&ed, &ft)
    });
    assert_eq!(l.below_widget_lines, below);
    let bot_idx = l.lines.iter().position(|line| line == &l.bottom_divider).unwrap();
    let below_idx = l.lines.iter().position(|line| line == "[hint: press Tab]").unwrap();
    let ft_idx = l
        .lines
        .iter()
        .position(|line| line.contains(&l.footer_lines[0]))
        .unwrap();
    assert!(bot_idx < below_idx && below_idx < ft_idx);
}

#[test]
fn below_widget_lines_yield_to_editor_dividers_and_footer_on_small_terminals() {
    let below = vec!["line 1".to_string(), "line 2".to_string(), "line 3".to_string()];
    let ed = EditorState::default();
    let ft = FooterState::default();
    let l = layout(LayoutInput {
        below_widget_lines: &below,
        terminal_height: 4,
        ..base_input(&ed, &ft)
    });
    assert!(l.below_widget_lines.is_empty());
    assert!(!l.editor_lines.is_empty());
    assert!(!l.footer_lines.is_empty());
}

#[test]
fn running_write_card_highlights_content_by_the_path_extension() {
    let code = "fn main() {\n    let x = 1;\n}\n";
    let mut known = RunningTool::new("write", "demo.rs", json!({"path": "demo.rs"}));
    known.append_chunk(code);
    let mut unknown = RunningTool::new("write", "", json!({}));
    unknown.append_chunk(code);

    let render = |tool: &RunningTool, theme: &Theme| {
        render_running_tool_widget(RunningToolWidgetInput {
            tool,
            theme,
            width: 60,
            tools_expanded: false,
        })
        .join("\n")
    };
    let structured = Theme {
        structured_tool_cards: true,
        ..Theme::default()
    };
    for theme in [Theme::default(), structured] {
        assert_ne!(
            render(&known, &theme),
            render(&unknown, &theme),
            "the extension decides the colors even with syntax_highlight_tools off"
        );
    }
    let known_text = crate::ui::block::strip_ansi(&render_test_widget(&known, false));
    assert!(known_text.contains("  2 │     let x = 1;"), "{known_text}");
}

#[test]
fn running_edit_card_shows_its_diff_at_the_lines_noted_before_it_ran() {
    let args = json!({"path": "/nonexistent/a.rs", "edits": [{"oldText": "old()", "newText": "new()"}]});
    let tool = RunningTool::start(crate::ui::interactive::ToolStartRequest {
        name: "edit".into(),
        args_summary: "a.rs".into(),
        arguments: args,
        file_change: Some(rho_harness_core::presentation::FileChange {
            overwrote_existing: true,
            edit_start_lines: vec![Some(7)],
            replaced_regions: vec![None],
        }),
    });

    let text = crate::ui::block::strip_ansi(&render_test_widget(&tool, false));

    assert!(text.contains("1 edit \u{b7} +1 -1"), "{text}");
    assert!(
        text.contains("7 \u{2502} - old()") && text.contains("7 \u{2502} + new()"),
        "{text}"
    );
    assert!(text.contains("Elapsed"), "{text}");
}
