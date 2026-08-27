//! Tests for the `ui::render` module.

use super::formatters::{
    format_bash_approval_card, format_edit_diff, format_session_status, format_thinking_block, format_write_preview,
};
use super::renderer::{format_tool_output_preview, tool_title_style, webfetch_content_kind};
use super::summary::{
    approval_heading, bash_approval_details, clean_command_paths, read_summary_parts, to_relative_path,
};
use super::types::{BashApproval, SessionStatus, ToolLine};
use crate::tools::bash_ast::RiskTier;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent, UiEvent};
use crate::ui::theme::Theme;

#[test]
fn interactive_renderer_emits_formatted_output_and_activity_events() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    let activity = renderer.start_spinner("thinking...");
    renderer.print_thinking_token("considering");
    activity.finish_and_clear();
    renderer.print_token("answer");
    renderer.flush();
    renderer.finish_tool_line(ToolLine {
        name: "read",
        arguments: &serde_json::json!({"path": "src/lib.rs"}),
        is_error: false,
        output: "contents",
        output_summary: "contents",
    });

    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    assert!(output.contains("considering"));
    assert!(output.contains("answer"));
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
}

#[test]
fn ordinary_bash_approval_shows_only_the_command() {
    let reasons = vec!["Writes output through file redirection".to_string()];
    let details = bash_approval_details(&BashApproval {
        command: "echo test > output.txt",
        tier: RiskTier::Mutating,
        reasons: &reasons,
    });

    assert_eq!(details, ["$ echo test > output.txt"]);
}

#[test]
fn high_risk_bash_approval_explains_the_risk() {
    let reasons = vec!["Discards uncommitted changes".to_string()];
    let details = bash_approval_details(&BashApproval {
        command: "git reset --hard",
        tier: RiskTier::HighRisk,
        reasons: &reasons,
    });

    assert_eq!(details, ["$ git reset --hard", "", "Discards uncommitted changes"]);
    assert_eq!(approval_heading(RiskTier::HighRisk), "High-risk bash command");
    assert_eq!(approval_heading(RiskTier::Mutating), "Bash command requires approval");
}

#[test]
fn bash_approval_cards_match_transcript_blocks() {
    let theme = Theme::default();
    let ordinary = format_bash_approval_card(
        &BashApproval {
            command: "cargo test",
            tier: RiskTier::Mutating,
            reasons: &[],
        },
        &theme,
        40,
    );
    assert!(ordinary.contains("Bash command requires approval"));
    assert!(ordinary.contains("$ cargo test"));
    assert!(ordinary.contains("\x1b[33m"));

    let reasons = vec!["Discards uncommitted changes".to_string()];
    let high_risk = format_bash_approval_card(
        &BashApproval {
            command: "git reset --hard",
            tier: RiskTier::HighRisk,
            reasons: &reasons,
        },
        &theme,
        40,
    );
    assert!(high_risk.contains("High-risk bash command"));
    assert!(high_risk.contains("! Discards uncommitted changes"));
    assert!(high_risk.contains("\x1b[31m"));
}

#[test]
fn read_summaries_show_explicit_line_ranges() {
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20})),
        ("src/lib.rs".to_string(), Some(":10-29".to_string()))
    );
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs"})),
        ("src/lib.rs".to_string(), None)
    );
}

#[test]
fn test_to_relative_path() {
    let cwd = std::env::current_dir().unwrap();
    let abs = cwd.join("src/main.rs");
    let rel = to_relative_path(abs.to_str().unwrap());
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_clean_command_paths() {
    let cwd = std::env::current_dir().unwrap();
    let cwd_str = cwd.to_str().unwrap();
    let cmd = format!("cat {cwd_str}/Cargo.toml");
    let cleaned = clean_command_paths(&cmd);
    assert_eq!(cleaned, "cat Cargo.toml");
}

#[test]
fn test_format_edit_diff_renders_removals_and_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let x = 1;",
                "newText": "let x = 2;\nlet y = 3;"
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains("```diff"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
    assert!(diff.contains("```"));
    assert!(diff.ends_with('\n'));
}

#[test]
fn test_format_write_preview_renders_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme).unwrap();
    assert!(preview.contains("```diff"));
    assert!(preview.contains("+ def main():"));
    assert!(preview.contains("+     print('hello')"));
    assert!(preview.contains("```"));
}

#[test]
fn error_tool_titles_use_terminal_red_without_dimming() {
    assert_eq!(tool_title_style(false).render().to_string(), "\x1b[1m");
    assert_eq!(tool_title_style(true).render().to_string(), "\x1b[1m\x1b[31m");
}

#[test]
fn webfetch_content_kind_uses_format_or_url_extension() {
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/page"})),
        "text"
    );
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/data.json"})),
        "json"
    );
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/file", "format": "pdf"})),
        "pdf"
    );
}

#[test]
fn tool_output_previews_are_bounded() {
    let output = (1..=10)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let preview = format_tool_output_preview(&output, "empty");
    assert!(preview.contains("line 1"));
    assert!(preview.contains("line 8"));
    assert!(!preview.contains("line 9"));
    assert!(preview.ends_with("... (2 more lines)"));
    assert_eq!(format_tool_output_preview("", "empty"), "empty");
}

#[test]
fn session_status_keeps_runtime_context_visible() {
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "claude-sonnet",
            provider: "anthropic",
            context: "27.4% (1M)",
            quota: Some("93% (3h22m)"),
            auto_approve: false,
        }),
        "claude-sonnet | 27.4% (1M) | 93% (3h22m)"
    );
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "qwen",
            provider: "ollama",
            context: "0% (376k)",
            quota: None,
            auto_approve: true,
        }),
        "qwen | 0% (376k)"
    );
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
    assert!(formatted.contains("analyzing the problem"));
    assert!(formatted.contains("checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with("\n\n"));
}
