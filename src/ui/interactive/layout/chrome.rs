use super::text::{SPINNER_FRAMES as FRAMES, truncate_to_width};
use crate::ui::interactive::{Activity, FooterState, QueueKind, QueuedMessage};

pub fn thinking_divider_style(thinking_level: Option<&str>) -> (&'static str, &'static str) {
    match thinking_level.unwrap_or("off") {
        "off" => ("\x1b[2m", "\x1b[0m"),
        "minimal" => ("\x1b[90m", "\x1b[0m"),
        "low" => ("\x1b[34m", "\x1b[0m"),
        "medium" => ("\x1b[36m", "\x1b[0m"),
        "high" => ("\x1b[35m", "\x1b[0m"),
        "xhigh" => ("\x1b[31m", "\x1b[0m"),
        "max" => ("\x1b[1;31m", "\x1b[0m"),
        _ => ("\x1b[2m", "\x1b[0m"),
    }
}

fn busy_top_divider(width: usize, label: &str, (act_label, spinner): (&str, char), style: &str, reset: &str) -> String {
    let act_tag = format!("── {spinner} {act_label} ");
    let act_len = act_tag.chars().count();
    let has_version = !label.is_empty();
    let ver_len = if has_version { label.chars().count() + 5 } else { 0 };

    if has_version && width > act_len + ver_len {
        let middle = width - act_len - ver_len;
        format!("{style}{act_tag}{} {label} ───{reset}", "─".repeat(middle))
    } else if width >= act_len + 3 {
        let trail = width - act_len;
        format!("{style}{act_tag}{}{reset}", "─".repeat(trail))
    } else if width >= 7 {
        let trail = width - 5;
        format!("{style}── {spinner} {}{reset}", "─".repeat(trail))
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
    if let Some(act) = activity {
        busy_top_divider(width, label, act, style, reset)
    } else if !label.is_empty() && width >= label.len() + 6 {
        let lead = width - label.len() - 5;
        format!("{style}{} {label} ───{reset}", "─".repeat(lead))
    } else {
        format!("{style}{}{reset}", "─".repeat(width))
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
