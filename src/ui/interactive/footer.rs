use std::path::{Path, PathBuf};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::FooterState;

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

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() {
            let head_file = git_dir.join("HEAD");
            if let Ok(head_content) = std::fs::read_to_string(head_file) {
                let trimmed = head_content.trim();
                if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                    return Some(branch.to_string());
                }
            }
            break;
        } else if git_dir.is_file()
            && let Ok(content) = std::fs::read_to_string(git_dir)
            && let Some(gitdir_path) = content.trim().strip_prefix("gitdir:")
        {
            let gitdir = PathBuf::from(gitdir_path.trim());
            let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
            let head_file = resolved.join("HEAD");
            if let Ok(head_content) = std::fs::read_to_string(head_file) {
                let trimmed = head_content.trim();
                if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                    return Some(branch.to_string());
                }
            }
            break;
        }
        curr = dir.parent();
    }

    let mut cmd = std::process::Command::new("git");
    cmd.arg("branch").arg("--show-current");
    cmd.current_dir(cwd);
    if let Ok(output) = cmd.output()
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
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
    let clean = crate::ui::block::ANSI_PATTERN.replace_all(content, "");
    UnicodeWidthStr::width(clean.as_ref())
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

pub fn format_top_line(footer: &FooterState, width: usize) -> String {
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

    let status = footer
        .quota
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| footer.extra_status.as_deref().filter(|s| !s.is_empty()));

    match status {
        Some(text) => fit_right_aligned(&pwd, &sanitize_status_text(text), width),
        None => truncate_with_ellipsis(&pwd, width),
    }
}

pub fn format_stats_line(footer: &FooterState, width: usize) -> String {
    let mut parts = Vec::new();
    if footer.total_input_tokens > 0 {
        parts.push(format!("↑{}", format_tokens(footer.total_input_tokens)));
    }
    if footer.total_output_tokens > 0 {
        parts.push(format!("↓{}", format_tokens(footer.total_output_tokens)));
    }
    if footer.total_cache_read_tokens > 0 {
        parts.push(format!("R{}", format_tokens(footer.total_cache_read_tokens)));
    }
    if footer.total_cache_write_tokens > 0 {
        parts.push(format!("W{}", format_tokens(footer.total_cache_write_tokens)));
    }
    if let Some(cost) = footer.total_cost
        && cost > 0.0
    {
        parts.push(format!("${cost:.3}"));
    }

    let context_percent_str = match footer.context_percent {
        Some(percent) => {
            if percent < 0.05 && footer.total_input_tokens > 0 {
                "0.1%".to_string()
            } else if (percent.fract() * 10.0).round() == 0.0 {
                format!("{percent:.0}%")
            } else {
                format!("{percent:.1}%")
            }
        }
        None => {
            if let Some(context_str) = &footer.context {
                context_str.clone()
            } else {
                "?".to_string()
            }
        }
    };

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

    if let Some(speed) = footer.tokens_per_second {
        parts.push(format!("@{speed:.1}t/s"));
    }

    let left = parts.join(" ");

    let model_id = if footer.model.is_empty() {
        "no-model"
    } else {
        &footer.model
    };

    let model_details = if let Some(thinking) = &footer.thinking_level
        && !thinking.is_empty()
        && thinking != "off"
    {
        format!("{model_id} • {thinking}")
    } else {
        model_id.to_string()
    };

    let right = if footer.hidden_status_count > 0 {
        format!("{} • {model_details}", footer.hidden_status_count)
    } else {
        model_details
    };

    fit_right_aligned(&left, &right, width)
}

pub fn format_footer_lines(footer: &FooterState, width: usize) -> Vec<String> {
    vec![format_top_line(footer, width), format_stats_line(footer, width)]
}

#[cfg(test)]
mod tests {
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
            context_percent: Some(1.2),
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
        assert!(line.contains("1.2%/200k"));
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
}
