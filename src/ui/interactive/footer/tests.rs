use super::super::FooterState;
use super::lines::{
    format_footer_lines, format_model_details, format_percent_value, format_stats_line, format_top_line,
};
use super::path::abbreviate_home;
use super::text::{fit_right_aligned, format_tokens, sanitize_status_text};
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
        (131_072, "128k"),
        (200_000, "200k"),
        (262_144, "256k"),
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
        provider: "gemini".into(),
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
    for token in ["↑1.2k", "↓450", "R10k", "W2.0k", "$0.012", "1%/200k", "@258t/s"] {
        assert!(line.contains(token));
    }
    assert!(line.ends_with("gemini/gemini-3.8-flash/medium"));
}

#[test]
fn stats_line_clamps_sub_one_tokens_per_second() {
    let footer = FooterState {
        tokens_per_second: Some(0.4),
        ..sample_stats_footer()
    };
    let line = format_stats_line(&footer, 80);
    assert!(line.contains("@1t/s"));
}

#[test]
fn stats_line_omits_zero_cache_tokens() {
    let footer = FooterState {
        total_cache_read_tokens: 0,
        total_cache_write_tokens: 0,
        ..sample_stats_footer()
    };
    let line = format_stats_line(&footer, 80);
    assert!(!line.contains('R'));
    assert!(!line.contains('W'));
}

#[test]
fn format_footer_lines_returns_two_lines() {
    let lines = format_footer_lines(&sample_stats_footer(), 80, None);
    assert_eq!(lines.len(), 2);
}

#[test]
fn top_line_prefers_ephemeral_message_over_quota() {
    let footer = FooterState {
        activity: Activity::Idle,
        cwd: Some("/Users/alice/project".into()),
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80, Some("Model: gpt-4o (openai)"));
    assert!(line.ends_with("Model: gpt-4o (openai)"));
    assert!(!line.contains("5h: 80%"));
}

#[test]
fn top_line_returns_to_quota_after_message_clears() {
    let footer = FooterState {
        activity: Activity::Idle,
        cwd: Some("/work".into()),
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    assert!(format_top_line(&footer, 80, None).ends_with("5h: 80%"));
}

#[test]
fn ephemeral_message_flattens_newlines_to_single_slot() {
    let footer = FooterState {
        activity: Activity::Idle,
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80, Some("Steering queued\nat tool boundary"));
    assert!(line.contains("Steering queued at tool boundary"));
    assert!(!line.contains('\n'));
}

#[test]
fn blank_message_falls_back_to_persistent_status() {
    let footer = FooterState {
        activity: Activity::Idle,
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    assert!(format_top_line(&footer, 80, Some("   ")).ends_with("5h: 80%"));
}

#[test]
fn format_model_details_formats_provider_model_and_thinking() {
    let mut footer = FooterState {
        provider: "gemini".into(),
        model: "gemini-2.5-flash".into(),
        thinking_level: None,
        ..FooterState::default()
    };
    assert_eq!(format_model_details(&footer), "gemini/gemini-2.5-flash/off");

    footer.thinking_level = Some("high".into());
    assert_eq!(format_model_details(&footer), "gemini/gemini-2.5-flash/high");

    footer.provider.clear();
    assert_eq!(format_model_details(&footer), "gemini-2.5-flash/high");

    footer.model.clear();
    assert_eq!(format_model_details(&footer), "no-model/high");
}

#[test]
fn format_percent_value_formats_whole_number() {
    assert_eq!(format_percent_value(30.9), "30%");
    assert_eq!(format_percent_value(0.0), "0%");
    assert_eq!(format_percent_value(1.2), "1%");
    assert_eq!(format_percent_value(100.0), "100%");
}
