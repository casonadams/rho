pub mod path;
pub mod text;

#[cfg(test)]
mod tests;

pub use path::{abbreviate_home, get_git_branch};
pub use text::{
    fit_right_aligned, format_tokens, sanitize_status_text, truncate_to_width, truncate_with_ellipsis, visible_width,
};

use std::path::PathBuf;

use super::FooterState;

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
            if footer.context_window > 0 {
                "0%".to_string()
            } else if let Some(context_str) = &footer.context {
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
