mod ephemeral;

use super::*;
use crate::ui::interactive::Activity;
use std::path::Path;

#[test]
fn format_tokens_matches_status_line_breakpoints() {
    let cases = [
        (0, "0"),
        (999, "999"),
        (1_000, "1.0k"),
        (1_234, "1.2k"),
        (9_999, "10.0k"),
        (10_000, "10k"),
        (128_000, "128k"),
        (200_000, "200k"),
        (1_000_000, "1M"),
        (2_500_000, "3M"),
    ];
    for (tokens, expected) in cases {
        assert_eq!(format_tokens(tokens), expected);
    }
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
        model: "gemini-3.8-flash".into(),
        thinking_level: None,
        cwd: Some("/Users/alice/project".into()),
        git_branch: Some("main".into()),
        session_name: Some("auth-feature".into()),
        quota: Some("80% (3h22m)".into()),
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80, None);
    assert!(line.contains("(main)"));
    assert!(line.contains("• auth-feature"));
    assert!(line.ends_with("80% (3h22m)"));
}

fn sample_stats_footer() -> FooterState {
    FooterState {
        activity: Activity::Idle,
        model: "gemini-3.8-flash".into(),
        thinking_level: Some("medium".into()),
        total_input_tokens: 1_200,
        total_output_tokens: 450,
        total_cache_read_tokens: 10_000,
        total_cache_write_tokens: 2_000,
        total_cost: Some(0.012),
        context_percent: Some(1.2),
        context_window: 200_000,
        tokens_per_second: Some(258.0),
        ..FooterState::default()
    }
}

#[test]
fn stats_line_formats_usage_and_model() {
    let line = format_stats_line(&sample_stats_footer(), 80);
    for token in ["↑1.2k", "↓450", "R10k", "W2.0k", "$0.012", "1.2%/200k", "@258t/s"] {
        assert!(line.contains(token));
    }
    assert!(line.ends_with("gemini-3.8-flash • medium"));
}

#[test]
fn stats_line_clamps_sub_one_tokens_per_second() {
    let footer = FooterState {
        model: "llama".into(),
        tokens_per_second: Some(0.3),
        ..FooterState::default()
    };
    let line = format_stats_line(&footer, 80);
    assert!(line.contains("@1t/s"));

    let zero_footer = FooterState {
        model: "llama".into(),
        tokens_per_second: Some(0.0),
        ..FooterState::default()
    };
    let zero_line = format_stats_line(&zero_footer, 80);
    assert!(!zero_line.contains("t/s"));
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
    let lines = format_footer_lines(&footer, 80, None);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("/work"));
    assert!(lines[1].contains("test-model"));
}

#[test]
fn get_git_branch_discovers_branch_in_repo() {
    let temp = tempfile::tempdir().unwrap();
    let git_dir = temp.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature-branch\n").unwrap();
    assert_eq!(get_git_branch(temp.path()), Some("feature-branch".into()));

    let nested = temp.path().join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(get_git_branch(&nested), Some("feature-branch".into()));

    let non_git = tempfile::tempdir().unwrap();
    assert_eq!(get_git_branch(non_git.path()), None);
}
