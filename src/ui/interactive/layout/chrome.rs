use super::text::SPINNER_FRAMES as FRAMES;
use crate::ui::interactive::footer::segments::context_threshold_color;
use crate::ui::interactive::footer::{format_tokens, truncate_to_width};
use crate::ui::interactive::{Activity, FooterState, QueueKind, QueuedMessage};
use rho_harness_core::config::ContextLineMode;
use rho_harness_core::thinking::ThinkingLevel;

pub fn thinking_divider_style(thinking_level: Option<&str>) -> (&'static str, &'static str) {
    let style = match ThinkingLevel::from_setting(thinking_level) {
        ThinkingLevel::Off => "\x1b[2m",
        ThinkingLevel::Minimal => "\x1b[90m",
        ThinkingLevel::Low => "\x1b[34m",
        ThinkingLevel::Medium => "\x1b[36m",
        ThinkingLevel::High => "\x1b[35m",
        ThinkingLevel::XHigh => "\x1b[31m",
        ThinkingLevel::Max => "\x1b[1;31m",
    };
    (style, "\x1b[0m")
}

fn format_act_tag(act: Option<(&str, char)>) -> Option<String> {
    act.map(|(act_label, spinner)| format!("── {spinner} {act_label} "))
}

fn tagged_top_divider(width: usize, label: &str, tag: &str, spinner: Option<char>, style: &str, reset: &str) -> String {
    let tag_len = tag.chars().count();
    let has_version = !label.is_empty();
    let ver_len = if has_version { label.chars().count() + 5 } else { 0 };

    if has_version && width > tag_len + ver_len {
        let middle = width - tag_len - ver_len;
        format!("{style}{tag}{} {label} ───{reset}", "─".repeat(middle))
    } else if width >= tag_len + 3 {
        let trail = width - tag_len;
        format!("{style}{tag}{}{reset}", "─".repeat(trail))
    } else if let Some(sp) = spinner
        && width >= 7
    {
        let trail = width - 5;
        format!("{style}── {sp} {}{reset}", "─".repeat(trail))
    } else {
        format!("{style}{}{reset}", "─".repeat(width))
    }
}

fn idle_top_divider(width: usize, label: &str, style: &str, reset: &str) -> String {
    if !label.is_empty() && width >= label.len() + 6 {
        let lead = width - label.len() - 5;
        format!("{style}{} {label} ───{reset}", "─".repeat(lead))
    } else {
        format!("{style}{}{reset}", "─".repeat(width))
    }
}

pub fn active_activity_status(footer: &FooterState, spinner_frame: usize) -> Option<(&'static str, char)> {
    if matches!(footer.activity, Activity::Idle) && footer.running_tool.as_deref().is_none() {
        None
    } else {
        let label = match footer.activity {
            Activity::Compacting => "compacting",
            _ => "working",
        };
        let spinner = FRAMES[spinner_frame % FRAMES.len()];
        Some((label, spinner))
    }
}

pub fn top_divider(width: usize, label: &str, activity: Option<(&str, char)>, style: &str, reset: &str) -> String {
    let spinner = activity.map(|(_, sp)| sp);
    if let Some(tag) = format_act_tag(activity) {
        tagged_top_divider(width, label, &tag, spinner, style, reset)
    } else {
        idle_top_divider(width, label, style, reset)
    }
}

pub fn modal_banner_title(modal: &crate::ui::interactive::ModalState) -> &str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Input { prompt_label } => match prompt_label.as_str() {
            "args" => "edit",
            other => other,
        },
        crate::ui::interactive::ModalMode::Select => &modal.title,
    }
}

pub fn modal_top_divider(width: usize, title: &str, style: &str, reset: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return format!("{style}{}{reset}", "─".repeat(width));
    }
    let label = format!(" {title} ");
    if width >= label.len() + 4 {
        let trail = width.saturating_sub(label.len() + 2);
        format!("{style}──{label}{}{reset}", "─".repeat(trail))
    } else {
        format!("{style}{}{reset}", "─".repeat(width))
    }
}

pub fn queued_lines_text(queued: &[QueuedMessage], width: usize) -> Vec<String> {
    if queued.is_empty() || width < 12 {
        return Vec::new();
    }
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";
    let accent = "\x1b[36m";
    let mut lines = Vec::new();
    for item in queued {
        let kind_label = match item.kind {
            QueueKind::Steering if item.text.starts_with('/') => "Command",
            QueueKind::Steering => "Steering",
            QueueKind::FollowUp => "Follow-up",
        };
        let text = format!("{dim}⇣ {kind_label}: {}{reset}", item.text.replace('\n', " "));
        lines.push(truncate_to_width(&text, width));
    }
    let hint = format!("{dim}↳ {accent}Alt+↑{reset}{dim} to edit queued messages{reset}");
    lines.push(truncate_to_width(&hint, width));
    lines
}

pub fn working_line_text(footer: &FooterState, spinner_frame: usize, width: usize) -> String {
    let activity = &footer.activity;
    let running_tool = footer.running_tool.as_deref();
    if (matches!(activity, Activity::Idle) && running_tool.is_none()) || width < 3 {
        return String::new();
    }
    let spinner = FRAMES[spinner_frame % FRAMES.len()];
    let accent = "\x1b[36m";
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";
    let label = match activity {
        Activity::Compacting => "compacting",
        _ => "working",
    };
    let full = format!(" {accent}{spinner}{reset} {dim}{label}{reset}");
    truncate_to_width(&full, width)
}

fn context_line_label(mode: ContextLineMode, pct: f64, context_window: usize) -> Option<String> {
    match mode {
        ContextLineMode::Off | ContextLineMode::Percentage => None,
        ContextLineMode::Annotated => Some(format!(" {}% ", pct.floor() as u64)),
        ContextLineMode::Embedded => {
            if context_window > 0 {
                Some(format!(
                    " {}% / {} ",
                    pct.floor() as u64,
                    format_tokens(context_window as u64)
                ))
            } else {
                Some(format!(" {}% ", pct.floor() as u64))
            }
        }
    }
}

pub fn bottom_divider(width: usize, style: &str, reset: &str, footer: &FooterState) -> String {
    let mode = footer
        .status_bar_config
        .as_ref()
        .and_then(|c| c.context_line)
        .unwrap_or(ContextLineMode::Off);
    if matches!(mode, ContextLineMode::Off) || width == 0 {
        return format!("{style}{}{reset}", "─".repeat(width));
    }

    let pct = footer.context_percent.unwrap_or(0.0).clamp(0.0, 100.0);
    let used_cols = (((pct / 100.0) * width as f64).round() as usize).min(width);
    let mut chars = vec!['─'; width];

    if let Some(label) = context_line_label(mode, pct, footer.context_window) {
        let label_chars: Vec<char> = label.chars().collect();
        if width >= label_chars.len() + 4 {
            let start = width - label_chars.len() - 3;
            chars[start..start + label_chars.len()].copy_from_slice(&label_chars);
        }
    }

    let used_part: String = chars[..used_cols].iter().collect();
    let rest_part: String = chars[used_cols..].iter().collect();
    let used_color = if pct >= 50.0 {
        context_threshold_color(pct)
    } else {
        style
    };
    format!("{used_color}{used_part}\x1b[2m{rest_part}{reset}")
}
