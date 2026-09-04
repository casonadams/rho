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

pub fn top_divider(width: usize, label: &str, (style, reset): (&str, &str)) -> String {
    if width >= label.len() + 4 {
        let lead = width - label.len() - 3;
        format!("{style}{}{label}{}{reset}", "─".repeat(lead), "─".repeat(3))
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
    let full = format!("{accent}{spinner}{reset} {dim}Working...{reset}");
    truncate_to_width(&full, width)
}
