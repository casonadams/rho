use chrono::{DateTime, Utc};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn strip_ansi(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn visible_width(content: &str) -> usize {
    let clean = strip_ansi(content);
    UnicodeWidthStr::width(clean.replace('\r', "").as_str())
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

pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 100_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count.is_multiple_of(1_000_000) {
        format!("{}M", count / 1_000_000)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes}B")
    }
}

pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{secs}s")
    } else {
        format!("{}ms", duration.as_millis())
    }
}

pub fn format_duration_ms(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    let millis = duration_ms % 1000;
    if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds > 0 {
        if millis == 0 {
            format!("{seconds}s")
        } else {
            format!("{seconds}.{millis:03}s")
        }
    } else {
        format!("{duration_ms}ms")
    }
}

pub fn format_relative_time(time: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(time);
    let secs = diff.num_seconds();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        let dur = std::time::Duration::from_secs((secs as u64 / 60) * 60);
        let formatted = humantime::format_duration(dur);
        format!("{formatted} ago")
    } else if secs < 86400 {
        let dur = std::time::Duration::from_secs((secs as u64 / 3600) * 3600);
        let formatted = humantime::format_duration(dur);
        format!("{formatted} ago")
    } else if secs < 2592000 {
        let dur = std::time::Duration::from_secs((secs as u64 / 86400) * 86400);
        let formatted = humantime::format_duration(dur).to_string();
        let short = formatted.replace("days", "d").replace("day", "d");
        format!("{short} ago")
    } else {
        time.format("%Y-%m-%d").to_string()
    }
}
