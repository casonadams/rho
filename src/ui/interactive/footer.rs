use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

use super::FooterState;

pub mod path {
    pub use super::{abbreviate_home, get_git_branch};
}

pub mod text {
    pub use super::{
        fit_right_aligned, format_tokens, sanitize_status_text, truncate_to_width, truncate_with_ellipsis,
        visible_width,
    };
}

pub fn abbreviate_home(cwd: &Path, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return cwd.display().to_string();
    };
    if cwd == home {
        return "~".to_string();
    }
    if let Ok(rel) = cwd.strip_prefix(home) {
        let rel_str = rel.to_string_lossy();
        if rel_str.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rel_str}");
    }
    cwd.display().to_string()
}

fn branch_from_head_file(head_file: &Path) -> Option<String> {
    let head_content = std::fs::read_to_string(head_file).ok()?;
    head_content.trim().strip_prefix("ref: refs/heads/").map(str::to_string)
}

fn branch_from_git_dir(dir: &Path, git_dir: &Path) -> Option<String> {
    let head_file = git_dir.join("HEAD");
    if git_dir.is_dir() {
        return branch_from_head_file(&head_file);
    }
    if git_dir.is_file() {
        let content = std::fs::read_to_string(git_dir).ok()?;
        let gitdir_path = content.trim().strip_prefix("gitdir:")?;
        let gitdir = PathBuf::from(gitdir_path.trim());
        let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
        return branch_from_head_file(&resolved.join("HEAD"));
    }
    None
}

fn branch_from_git_process(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("branch")
        .arg("--show-current")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() || git_dir.is_file() {
            if let Some(branch) = branch_from_git_dir(dir, &git_dir) {
                return Some(branch);
            }
            break;
        }
        curr = dir.parent();
    }

    branch_from_git_process(cwd)
}

pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else {
        format!("{}M", (count as f64 / 1_000_000.0).round() as u64)
    }
}

pub fn sanitize_status_text(text: &str) -> String {
    let single_line = text
        .chars()
        .map(|c| if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c })
        .collect::<String>();
    let mut words = single_line.split_whitespace();
    let mut result = String::new();
    if let Some(first) = words.next() {
        result.push_str(first);
        for word in words {
            result.push(' ');
            result.push_str(word);
        }
    }
    result
}

pub fn visible_width(content: &str) -> usize {
    crate::ui::block::visible_width(content)
}

pub fn truncate_to_width(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_string();
    }

    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result
}

pub fn truncate_with_ellipsis(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_string();
    }
    if width <= 3 {
        return truncate_to_width(value, width);
    }
    let target = width - 3;
    let truncated = truncate_to_width(value, target);
    format!("{truncated}...")
}

pub fn fit_right_aligned(left: &str, right: &str, width: usize) -> String {
    let right_width = visible_width(right);
    let safe_right = if right_width > width {
        truncate_to_width(right, width)
    } else {
        right.to_string()
    };
    let safe_right_width = visible_width(&safe_right);

    let left_width = visible_width(left);
    if left_width + safe_right_width + 2 <= width {
        let padding = width.saturating_sub(left_width + safe_right_width);
        return format!("{left}{}{safe_right}", " ".repeat(padding));
    }

    let available_left = width.saturating_sub(safe_right_width + 2);
    let truncated_left = if available_left > 0 {
        truncate_with_ellipsis(left, available_left)
    } else {
        String::new()
    };
    let truncated_left_width = visible_width(&truncated_left);
    let padding = width.saturating_sub(truncated_left_width + safe_right_width);
    format!("{truncated_left}{}{safe_right}", " ".repeat(padding))
}

fn resolve_status_text(footer: &FooterState, system_message: Option<&str>) -> Option<String> {
    system_message
        .filter(|s| !s.trim().is_empty())
        .or(footer.quota.as_deref().filter(|s| !s.is_empty()))
        .or(footer.extra_status.as_deref().filter(|s| !s.is_empty()))
        .map(sanitize_status_text)
}

pub fn format_top_line(footer: &FooterState, width: usize, system_message: Option<&str>) -> String {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let cwd_path = footer
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut pwd = abbreviate_home(&cwd_path, home.as_deref());
    if let Some(branch) = &footer.git_branch
        && !branch.is_empty()
    {
        pwd.push_str(&format!(" ({branch})"));
    }
    if let Some(name) = &footer.session_name
        && !name.is_empty()
    {
        pwd.push_str(&format!(" • {name}"));
    }

    match resolve_status_text(footer, system_message) {
        Some(text) => fit_right_aligned(&pwd, &text, width),
        None => truncate_with_ellipsis(&pwd, width),
    }
}

fn push_cache_parts(footer: &FooterState, parts: &mut Vec<String>) {
    if footer.total_cache_read_tokens > 0 {
        parts.push(format!("R{}", format_tokens(footer.total_cache_read_tokens)));
    }
    if footer.total_cache_write_tokens > 0 {
        parts.push(format!("W{}", format_tokens(footer.total_cache_write_tokens)));
    }
}

fn collect_token_parts(footer: &FooterState, parts: &mut Vec<String>) {
    if footer.total_input_tokens > 0 {
        parts.push(format!("↑{}", format_tokens(footer.total_input_tokens)));
    }
    if footer.total_output_tokens > 0 {
        parts.push(format!("↓{}", format_tokens(footer.total_output_tokens)));
    }
    push_cache_parts(footer, parts);
    if let Some(cost) = footer.total_cost.filter(|c| *c > 0.0) {
        parts.push(format!("${cost:.3}"));
    }
}

fn format_context_percent(footer: &FooterState) -> String {
    match footer.context_percent {
        Some(percent) => format_percent_value(percent, footer.total_input_tokens),
        None if footer.context_window > 0 => "0%".to_string(),
        None => footer.context.clone().unwrap_or_else(|| "?".to_string()),
    }
}

fn format_percent_value(percent: f64, total_input: u64) -> String {
    if percent < 0.05 && total_input > 0 {
        return "0.1%".to_string();
    }
    if (percent.fract() * 10.0).round() == 0.0 {
        format!("{percent:.0}%")
    } else {
        format!("{percent:.1}%")
    }
}

fn push_context_part(footer: &FooterState, parts: &mut Vec<String>) {
    let context_percent_str = format_context_percent(footer);
    if footer.context_window > 0 {
        let window_str = format_tokens(footer.context_window as u64);
        if context_percent_str.contains('/') || context_percent_str.contains("tokens") {
            parts.push(context_percent_str);
        } else {
            parts.push(format!("{context_percent_str}/{window_str}"));
        }
    } else if !context_percent_str.is_empty() && context_percent_str != "?" {
        parts.push(context_percent_str);
    }
}

fn push_speed_part(footer: &FooterState, parts: &mut Vec<String>) {
    if let Some(speed) = footer.tokens_per_second
        && speed > 0.0
    {
        let tps = (speed.round() as u64).max(1);
        parts.push(format!("@{tps}t/s"));
    }
}

fn format_model_details(footer: &FooterState) -> String {
    let model_id = if footer.model.is_empty() {
        "no-model"
    } else {
        &footer.model
    };
    match &footer.thinking_level {
        Some(thinking) if !thinking.is_empty() && thinking != "off" => format!("{model_id} • {thinking}"),
        _ => model_id.to_string(),
    }
}

pub fn format_stats_line(footer: &FooterState, width: usize) -> String {
    let mut parts = Vec::new();
    collect_token_parts(footer, &mut parts);
    push_context_part(footer, &mut parts);
    push_speed_part(footer, &mut parts);
    let left = parts.join(" ");

    let model_details = format_model_details(footer);
    let right = if footer.hidden_status_count > 0 {
        format!("{} • {model_details}", footer.hidden_status_count)
    } else {
        model_details
    };

    fit_right_aligned(&left, &right, width)
}

pub fn format_footer_lines(footer: &FooterState, width: usize, system_message: Option<&str>) -> Vec<String> {
    vec![
        format_top_line(footer, width, system_message),
        format_stats_line(footer, width),
    ]
}

#[cfg(test)]
mod tests {
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
}
