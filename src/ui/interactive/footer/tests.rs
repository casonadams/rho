use super::*;
use crate::ui::interactive::Activity;
use std::path::Path;

#[test]
fn format_tokens_matches_status_line_breakpoints() {
    assert_eq!(format_tokens(0), "0");
    assert_eq!(format_tokens(999), "999");
    assert_eq!(format_tokens(1_000), "1.0k");
    assert_eq!(format_tokens(1_234), "1.2k");
    assert_eq!(format_tokens(9_999), "10.0k");
    assert_eq!(format_tokens(10_000), "10k");
    assert_eq!(format_tokens(128_000), "128k");
    assert_eq!(format_tokens(200_000), "200k");
    assert_eq!(format_tokens(1_000_000), "1M");
    assert_eq!(format_tokens(2_500_000), "3M");
}

#[test]
fn abbreviate_home_replaces_prefix_with_tilde() {
    let home = Path::new("/Users/alice");
    assert_eq!(
        abbreviate_home(Path::new("/Users/alice/projects/rho"), Some(home)),
        "~/projects/rho"
    );
    assert_eq!(abbreviate_home(Path::new("/Users/alice"), Some(home)), "~");
    assert_eq!(
        abbreviate_home(Path::new("/Users/alice-work/repo"), Some(home)),
        "/Users/alice-work/repo"
    );
}

#[test]
fn fit_right_aligned_pads_and_truncates_left() {
    assert_eq!(fit_right_aligned("left", "right", 20), "left           right");
    assert_eq!(
        fit_right_aligned("very-long-left-side-text-here", "right", 20),
        "very-long-...  right"
    );
}

#[test]
fn sanitize_status_collapses_whitespace() {
    assert_eq!(sanitize_status_text("  hello \n\t  world  \r\n"), "hello world");
}

#[test]
fn top_line_contains_cwd_branch_session_and_quota() {
    let footer = FooterState {
        activity: Activity::Idle,
        model: "claude-3-7-sonnet".into(),
        thinking_level: None,
        cwd: Some("/Users/alice/project".into()),
        git_branch: Some("main".into()),
        session_name: Some("auth-feature".into()),
        quota: Some("80% (3h22m)".into()),
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80);
    assert!(line.contains("(main)"));
    assert!(line.contains("• auth-feature"));
    assert!(line.ends_with("80% (3h22m)"));
}

#[test]
fn stats_line_formats_usage_and_model() {
    let footer = FooterState {
        activity: Activity::Idle,
        model: "claude-3-7-sonnet".into(),
        thinking_level: Some("medium".into()),
        total_input_tokens: 1_200,
        total_output_tokens: 450,
        total_cache_read_tokens: 10_000,
        total_cache_write_tokens: 2_000,
        total_cost: Some(0.012),
        context_percent: Some(99.4),
        context_window: 200_000,
        tokens_per_second: Some(45.2),
        ..FooterState::default()
    };
    let line = format_stats_line(&footer, 80);
    assert!(line.contains("↑1.2k"));
    assert!(line.contains("↓450"));
    assert!(line.contains("R10k"));
    assert!(line.contains("W2.0k"));
    assert!(line.contains("$0.012"));
    assert!(line.contains("99.4%/200k"));
    assert!(line.contains("@45.2t/s"));
    assert!(line.ends_with("claude-3-7-sonnet • medium"));
}

#[test]
fn stats_line_with_hidden_status_count() {
    let footer = FooterState {
        activity: Activity::Idle,
        model: "gpt-4o".into(),
        hidden_status_count: 2,
        ..FooterState::default()
    };
    let line = format_stats_line(&footer, 80);
    assert!(line.ends_with("2 • gpt-4o"));
}

#[test]
fn format_footer_lines_emits_two_lines() {
    let footer = FooterState {
        model: "test-model".into(),
        cwd: Some("/work".into()),
        ..FooterState::default()
    };
    let lines = format_footer_lines(&footer, 80);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("/work"));
    assert!(lines[1].contains("test-model"));
}

#[test]
fn get_git_branch_discovers_branch_in_repo() {
    let cwd = std::env::current_dir().unwrap();
    let branch = get_git_branch(&cwd);
    assert!(branch.is_some());
}
