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
