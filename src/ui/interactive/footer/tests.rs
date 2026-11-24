use super::super::FooterState;
use super::lines::{
    format_footer_lines, format_model_details, format_percent_value, format_stats_line, format_top_line,
};
use super::path::abbreviate_home;
use super::text::{fit_right_aligned, sanitize_status_text, truncate_to_width};
use crate::ui::block::visible_width;
use crate::ui::interactive::Activity;
use std::path::Path;

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

#[test]
fn top_line_counts_running_subagents() {
    let footer = |running_agents| FooterState {
        cwd: Some("/tmp/p".into()),
        running_agents,
        ..FooterState::default()
    };
    assert!(!format_top_line(&footer(0), 80, None).contains('⟳'));
    assert!(format_top_line(&footer(1), 80, None).ends_with(" • ⟳ 1 agent (←)"));
    assert!(format_top_line(&footer(3), 80, None).ends_with(" • ⟳ 3 agents (←)"));
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
    for token in ["↑1.2k", "↓450", "R10.0k", "W2.0k", "$0.012", "1%/200k", "@258t/s"] {
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
fn model_details_name_the_account_and_shorten_long_ones() {
    let cases = [
        ("claude:me@work.com", "claude:me@work.com/opus/high"),
        (
            "claude:cason.adams.long@company.com",
            "claude:cason.adams…@company.com/opus/high",
        ),
        (
            "claude:x@very-long-subsidiary-name.example.com",
            "claude:x@very-long-subsidiary-…/opus/high",
        ),
    ];
    for (provider, expected) in cases {
        let footer = FooterState {
            provider: provider.into(),
            model: "opus".into(),
            thinking_level: Some("high".into()),
            ..FooterState::default()
        };
        assert_eq!(format_model_details(&footer), expected);
    }
}

#[test]
fn format_percent_value_formats_whole_number() {
    assert_eq!(format_percent_value(30.9), "30%");
    assert_eq!(format_percent_value(0.0), "0%");
    assert_eq!(format_percent_value(1.2), "1%");
    assert_eq!(format_percent_value(100.0), "100%");
}

#[test]
fn truncate_to_width_counts_only_visible_columns() {
    assert_eq!(truncate_to_width("\x1b[31mred\x1b[0m text", 5), "\x1b[31mred\x1b[0m t");
    assert_eq!(visible_width(&truncate_to_width("\x1b[2Kabcdef", 3)), 3);
}

#[tokio::test]
async fn get_git_branch_async_reads_head_without_blocking_worker() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/feature/async\n").unwrap();
    let branch = super::path::get_git_branch_async(dir.path().to_path_buf()).await;
    assert_eq!(branch.as_deref(), Some("feature/async"));
}

fn rich_segmented_footer() -> FooterState {
    FooterState {
        activity: Activity::Idle,
        provider: "anthropic".into(),
        model: "claude-sonnet-4-5".into(),
        thinking_level: Some("high".into()),
        cwd: Some("/tmp/rho-work".into()),
        git_branch: Some("main".into()),
        git_status: Some(super::super::GitStatusCounts {
            staged: 2,
            unstaged: 1,
            untracked: 3,
        }),
        session_id: Some("12345678-abcd-ef00".into()),
        session_name: Some("slice3".into()),
        hostname: Some("workstation".into()),
        active_duration_ms: 95_000,
        quota: Some("80%".into()),
        context_percent: Some(42.0),
        context_window: 200_000,
        total_input_tokens: 1_500,
        total_output_tokens: 500,
        total_cache_read_tokens: 3_000,
        total_cache_write_tokens: 200,
        total_cost: Some(0.025),
        tokens_per_second: Some(120.0),
        running_agents: 2,
        ..FooterState::default()
    }
}

#[test]
fn default_status_bar_config_matches_legacy_two_line_footer() {
    use rho_harness_core::config::{StatusBarConfig, StatusBarPreset};

    let base = sample_stats_footer();
    let legacy = format_footer_lines(&base, 80, Some("Saved"));

    let with_default_cfg = FooterState {
        status_bar_config: Some(StatusBarConfig {
            preset: Some(StatusBarPreset::Default),
            ..StatusBarConfig::default()
        }),
        ..base.clone()
    };
    assert_eq!(format_footer_lines(&with_default_cfg, 80, Some("Saved")), legacy);
}

#[test]
fn presets_render_single_line_with_expected_segments() {
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{StatusBarConfig, StatusBarPreset};

    let mut footer = rich_segmented_footer();
    let presets = [
        (
            StatusBarPreset::Minimal,
            &["/tmp/rho-work", "main", "slice3", "42.0%"][..],
        ),
        (
            StatusBarPreset::Compact,
            &["claude-sonnet-4-5", "main", "$0.025", "42.0%"],
        ),
        (
            StatusBarPreset::Full,
            &["workstation", "claude-sonnet-4-5", "⟳ 2", "⚡67%", "1m35s", "42.0%"],
        ),
        (
            StatusBarPreset::Nerd,
            &["workstation", "12345678", "↑1.5k", "↓500", "200k", "1m35s"],
        ),
        (StatusBarPreset::Ascii, &["git:main", "tok:2.0k", "$0.025", "42.0%"]),
    ];

    for (preset, needles) in presets {
        footer.status_bar_config = Some(StatusBarConfig {
            preset: Some(preset),
            ..StatusBarConfig::default()
        });
        let lines = format_footer_lines(&footer, 200, None);
        assert_eq!(lines.len(), 1, "preset {preset:?} should render 1 line");
        assert!(visible_width(&lines[0]) <= 200);
        let plain = strip_ansi(&lines[0]);
        for needle in needles {
            assert!(
                plain.contains(needle),
                "preset {preset:?} missing {needle:?} in {plain:?}"
            );
        }
    }
}

#[test]
fn custom_ordered_segments_and_separators() {
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{
        StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId, StatusBarSeparator,
    };

    let mut footer = rich_segmented_footer();
    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Custom),
        separator: Some(StatusBarSeparator::Pipe),
        icons: Some(StatusBarIconMode::Ascii),
        transparent: Some(true),
        left: Some(vec![StatusBarSegmentId::Hostname, StatusBarSegmentId::Session]),
        right: Some(vec![StatusBarSegmentId::Cost, StatusBarSegmentId::ContextPct]),
        ..StatusBarConfig::default()
    });
    let lines = format_footer_lines(&footer, 80, None);
    let plain = strip_ansi(&lines[0]);
    assert!(plain.contains("workstation │ 12345678"));
    assert!(plain.contains("$0.025 │ 42.0%"));
    assert!(!lines[0].contains("\x1b[48;"));
}

#[test]
fn separators_and_transparent_mode_behavior() {
    use super::format_footer_lines_with_theme;
    use anstyle::{Color, RgbColor, Style};
    use rho_harness_core::config::{ContextLineMode, StatusBarConfig, StatusBarPreset, StatusBarSeparator};

    let mut footer = rich_segmented_footer();
    let theme = crate::ui::theme::Theme {
        block_fill: Style::new().bg_color(Some(Color::Rgb(RgbColor(240, 236, 228)))),
        ..Default::default()
    };

    for (sep, expected_glyph) in [
        (StatusBarSeparator::Powerline, "\u{e0b0}"),
        (StatusBarSeparator::PowerlineThin, "\u{e0b1}"),
        (StatusBarSeparator::Pipe, "│"),
        (StatusBarSeparator::Ascii, ">"),
    ] {
        footer.status_bar_config = Some(StatusBarConfig {
            preset: Some(StatusBarPreset::Minimal),
            separator: Some(sep),
            transparent: Some(false),
            ..StatusBarConfig::default()
        });
        let lines = format_footer_lines_with_theme(&footer, 100, None, Some(&theme));
        assert!(
            lines[0].contains(expected_glyph),
            "sep {sep:?} missing {expected_glyph:?} in {:?}",
            lines[0]
        );
    }

    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Minimal),
        separator: Some(StatusBarSeparator::Powerline),
        transparent: Some(true),
        context_line: Some(ContextLineMode::Embedded),
        ..StatusBarConfig::default()
    });
    let transparent_lines = format_footer_lines_with_theme(&footer, 100, None, Some(&theme));
    assert!(!transparent_lines[0].contains("\x1b[48;"));
    assert!(!transparent_lines[0].contains('\u{e0b0}'));
    assert!(!transparent_lines[0].contains("42.0%"));
}

#[test]
fn git_dirty_counts_and_porcelain_parsing() {
    use super::super::GitStatusCounts;
    use super::path::parse_git_status_porcelain;
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId};

    let porcelain = "M  staged.rs\n M unstaged.rs\nMM both.rs\n?? untracked.txt\n";
    let counts = parse_git_status_porcelain(porcelain);
    assert_eq!(
        counts,
        GitStatusCounts {
            staged: 2,
            unstaged: 2,
            untracked: 1,
        }
    );
    assert!(counts.is_dirty());

    let mut footer = rich_segmented_footer();
    footer.git_status = Some(counts);
    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Custom),
        icons: Some(StatusBarIconMode::Ascii),
        transparent: Some(true),
        left: Some(vec![StatusBarSegmentId::Git]),
        right: Some(vec![]),
        ..StatusBarConfig::default()
    });
    let dirty_line = &format_footer_lines(&footer, 80, None)[0];
    assert!(dirty_line.contains("\x1b[33m"));
    assert!(strip_ansi(dirty_line).contains("git:main +2 *2 ?1"));

    footer.git_status = Some(GitStatusCounts::default());
    let clean_line = &format_footer_lines(&footer, 80, None)[0];
    assert!(clean_line.contains("\x1b[32m"));
    assert_eq!(strip_ansi(clean_line).trim(), "git:main");
}

#[test]
fn progressive_disclosure_drops_whole_segments_before_truncating() {
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{
        StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId, StatusBarSeparator,
    };

    let mut footer = rich_segmented_footer();
    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Custom),
        separator: Some(StatusBarSeparator::Pipe),
        icons: Some(StatusBarIconMode::Ascii),
        transparent: Some(true),
        left: Some(vec![StatusBarSegmentId::Hostname, StatusBarSegmentId::Session]),
        right: Some(vec![StatusBarSegmentId::Cost, StatusBarSegmentId::ContextPct]),
        ..StatusBarConfig::default()
    });

    // At width 34, "workstation │ 12345678" (22) + gap (1) + "$0.025 │ 42.0%" (12) = 35 > 34,
    // so tail right segment (42.0%) is dropped whole, keeping "$0.025" intact!
    let narrow = &format_footer_lines(&footer, 34, None)[0];
    let plain = strip_ansi(narrow);
    assert!(visible_width(narrow) <= 34);
    assert!(plain.contains("workstation"));
    assert!(plain.contains("12345678"));
    assert!(plain.contains("$0.025"));
    assert!(!plain.contains("42.0%"));
}

#[test]
fn model_compact_thinking_and_short_name_and_system_message() {
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{
        ModelSegmentOptions, StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId,
        StatusBarSegmentOptions,
    };

    let mut footer = rich_segmented_footer();
    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Custom),
        icons: Some(StatusBarIconMode::Ascii),
        transparent: Some(true),
        compact_thinking_level: Some(true),
        left: Some(vec![StatusBarSegmentId::Model]),
        right: Some(vec![]),
        segment_options: Some(StatusBarSegmentOptions {
            model: Some(ModelSegmentOptions {
                show_thinking_level: Some(true),
                short_name: Some(true),
                ..ModelSegmentOptions::default()
            }),
            ..StatusBarSegmentOptions::default()
        }),
        ..StatusBarConfig::default()
    });
    let lines = format_footer_lines(&footer, 80, Some("Model updated"));
    assert_eq!(lines.len(), 2);
    assert_eq!(strip_ansi(&lines[0]).trim(), "claude-sonnet-4-5 ◕");
    assert_eq!(lines[1], "Model updated");
}

#[test]
fn pr_segment_dot_separator_and_model_alias_names() {
    use std::collections::BTreeMap;

    use super::super::GitPrInfo;
    use super::path::{is_default_branch, parse_gh_pr_json, parse_github_repo};
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{
        ModelSegmentOptions, StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId,
        StatusBarSegmentOptions, StatusBarSeparator,
    };

    assert!(is_default_branch("main"));
    assert!(is_default_branch("master"));
    assert!(!is_default_branch("feat/status-bar"));

    assert_eq!(
        parse_github_repo("git@github.com:org/repo.git"),
        Some("org/repo".into())
    );
    assert_eq!(
        parse_github_repo("https://github.com/org/repo.git\n"),
        Some("org/repo".into())
    );
    assert_eq!(
        parse_github_repo("ssh://git@github.com/org/repo"),
        Some("org/repo".into())
    );
    assert_eq!(parse_github_repo("https://gitlab.com/org/repo.git"), None);

    let parsed = parse_gh_pr_json(r#"{"number": 72, "url": "https://github.com/org/repo/pull/72"}"#).unwrap();
    assert_eq!(
        parsed,
        GitPrInfo {
            number: 72,
            url: Some("https://github.com/org/repo/pull/72".into()),
        }
    );

    let mut names = BTreeMap::new();
    names.insert("gemini-4-neon".into(), "Neo4".into());

    let mut footer = rich_segmented_footer();
    footer.provider = "gemini".into();
    footer.model = "gemini-4-neon".into();
    footer.thinking_level = Some("medium".into());
    footer.git_pr = Some(parsed);
    footer.status_bar_config = Some(StatusBarConfig {
        preset: Some(StatusBarPreset::Custom),
        separator: Some(StatusBarSeparator::Dot),
        icons: Some(StatusBarIconMode::Unicode),
        transparent: Some(true),
        compact_thinking_level: Some(false),
        left: Some(vec![StatusBarSegmentId::Pr, StatusBarSegmentId::Model]),
        right: Some(vec![StatusBarSegmentId::ContextPct]),
        segment_options: Some(StatusBarSegmentOptions {
            model: Some(ModelSegmentOptions {
                show_thinking_level: Some(true),
                short_name: Some(true),
                names,
            }),
            ..StatusBarSegmentOptions::default()
        }),
        ..StatusBarConfig::default()
    });

    let lines = format_footer_lines(&footer, 100, None);
    let plain = strip_ansi(&lines[0]);
    assert!(plain.contains("PR #72"), "got: {plain}");
    assert!(plain.contains(" · "), "got: {plain}");
    assert!(plain.contains("Neo4 · ◒ med"), "got: {plain}");
    assert!(plain.contains("42.0%/200k"), "got: {plain}");
}
