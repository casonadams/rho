use std::path::{Path, PathBuf};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::FooterState;

#[cfg(test)]
mod tests;

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
