use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use chrono::Local;
use rho_harness_core::config::{
    GitSegmentOptions, ModelSegmentOptions, PathSegmentOptions, StatusBarIconMode, StatusBarSegmentId,
    StatusBarSegmentOptions, StatusBarSeparator, TimeFormat, TimeSegmentOptions,
};
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::thinking::ThinkingLevel;

use super::super::FooterState;
use super::path::abbreviate_home;
use super::text::{format_tokens, sanitize_status_text};
use crate::ui::theme::Theme;

mod join;
mod presets;

pub(crate) use join::format_segmented_status_bar;
use join::wrap_fg;
pub(crate) use presets::{is_default_status_bar, status_bar_uses_segment};

trait IconModePick {
    fn pick<T>(self, nerd: T, unicode: T, ascii: T) -> T;
}

impl IconModePick for StatusBarIconMode {
    fn pick<T>(self, nerd: T, unicode: T, ascii: T) -> T {
        match self {
            Self::Nerd => nerd,
            Self::Unicode => unicode,
            Self::Ascii => ascii,
        }
    }
}

struct ResolvedStatusBar {
    left: Vec<StatusBarSegmentId>,
    right: Vec<StatusBarSegmentId>,
    separator: StatusBarSeparator,
    icons: StatusBarIconMode,
    transparent: bool,
    compact_thinking_level: bool,
    options: StatusBarSegmentOptions,
}

pub(crate) struct ThinkingDisplay {
    pub icon: &'static str,
    pub label: &'static str,
    pub sgr: &'static str,
    pub dim: bool,
}

pub(crate) fn thinking_display(level: Option<&str>) -> ThinkingDisplay {
    let (icon, label, sgr, dim) = match ThinkingLevel::from_setting(level) {
        ThinkingLevel::Off => ("○", "off", "\x1b[2m", true),
        ThinkingLevel::Minimal => ("◔", "min", "\x1b[90m", false),
        ThinkingLevel::Low => ("◑", "low", "\x1b[34m", false),
        ThinkingLevel::Medium => ("◒", "med", "\x1b[36m", false),
        ThinkingLevel::High => ("◕", "high", "\x1b[35m", false),
        ThinkingLevel::XHigh => ("◉", "xhigh", "\x1b[31m", false),
        ThinkingLevel::Max => ("◉", "max", "\x1b[1;31m", false),
        ThinkingLevel::Auto => ("◐", "auto", "\x1b[33m", false),
    };
    ThinkingDisplay { icon, label, sgr, dim }
}

fn model_name_part(footer: &FooterState, short_name: bool, opts: Option<&ModelSegmentOptions>) -> String {
    if footer.model.is_empty() {
        return if short_name || footer.provider.is_empty() {
            "no-model".to_string()
        } else {
            format!("{}/no-model", footer.provider)
        };
    }
    let route = ModelRoute::new(&footer.provider, &footer.model);
    let bare = route.model.rsplit('/').next().unwrap_or(&route.model);
    if let Some(names) = opts.map(|o| &o.names) {
        let full = route.to_string();
        if let Some(alias) = names
            .get(&full)
            .or_else(|| names.get(route.model.as_str()))
            .or_else(|| names.get(bare))
        {
            return alias.clone();
        }
    }
    if short_name {
        bare.to_string()
    } else if route.provider().is_empty() {
        route.model
    } else {
        route.to_string()
    }
}

fn render_model_segment(
    footer: &FooterState,
    icons: StatusBarIconMode,
    compact_thinking: bool,
    opts: Option<&ModelSegmentOptions>,
) -> Option<String> {
    let short_name = opts.and_then(|o| o.short_name).unwrap_or(false);
    let show_thinking = opts.and_then(|o| o.show_thinking_level).unwrap_or(true);
    let prefix = icons.pick("\u{f06a9} ", "◆ ", "");
    let name = sanitize_status_text(&model_name_part(footer, short_name, opts));
    if !show_thinking {
        return Some(format!("{prefix}{name}"));
    }
    let ThinkingDisplay { icon, label: level, .. } = thinking_display(footer.thinking_level.as_deref());
    if compact_thinking {
        Some(format!("{prefix}{name} {icon}"))
    } else if matches!(icons, StatusBarIconMode::Ascii) {
        Some(format!("{prefix}{name}/{level}"))
    } else {
        Some(format!("{prefix}{name} · {icon} {level}"))
    }
}

fn sanitize_osc8_url(url: &str) -> Option<&str> {
    let valid_scheme = url.starts_with("https://") || url.starts_with("http://");
    let has_ctrl = url.bytes().any(|b| b < 0x20 || b == 0x7f);
    (valid_scheme && !has_ctrl).then_some(url)
}

fn render_pr_segment(footer: &FooterState, icons: StatusBarIconMode) -> Option<String> {
    let pr = footer.git_pr.as_ref()?;
    let label = icons.pick(
        format!("\u{f407} #{}", pr.number),
        format!("PR #{}", pr.number),
        format!("PR #{}", pr.number),
    );
    match pr.url.as_deref().and_then(sanitize_osc8_url) {
        Some(url) => Some(format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")),
        None => Some(label),
    }
}

fn split_clusters(text: &str) -> Vec<(&str, usize)> {
    let mut clusters = Vec::new();
    let mut start = 0;
    let mut width = 0;
    for (idx, ch) in text.char_indices() {
        let w = ch.width().unwrap_or(0);
        if idx > 0 && w > 0 {
            clusters.push((&text[start..idx], width));
            start = idx;
            width = w;
        } else {
            width += w;
        }
    }
    if start < text.len() {
        clusters.push((&text[start..], width));
    }
    clusters
}

fn truncate_path_left(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.width() <= max_len {
        return text.to_string();
    }
    let e_len = ellipsis.width();
    if max_len <= e_len {
        return ellipsis.chars().take(max_len).collect();
    }
    let keep_width = max_len - e_len;
    let clusters = split_clusters(text);
    let mut used = 0;
    let mut start_idx = clusters.len();
    for (idx, &(_, w)) in clusters.iter().enumerate().rev() {
        if used + w > keep_width {
            break;
        }
        used += w;
        start_idx = idx;
    }
    let mut out = String::from(ellipsis);
    for &(cluster, _) in &clusters[start_idx..] {
        out.push_str(cluster);
    }
    out
}

fn render_path_segment(
    footer: &FooterState,
    icons: StatusBarIconMode,
    opts: Option<&PathSegmentOptions>,
) -> Option<String> {
    let raw = footer.cwd.as_deref().filter(|s| !s.is_empty())?;
    let abbreviate = opts.and_then(|o| o.abbreviate).unwrap_or(true);
    let mut display = if abbreviate {
        let home = rho_harness_core::config::home_dir();
        abbreviate_home(&PathBuf::from(raw), home.as_deref())
    } else {
        raw.to_string()
    };
    display = sanitize_status_text(&display);
    if let Some(max_len) = opts.and_then(|o| o.max_length) {
        display = truncate_path_left(&display, max_len, icons.pick("…", "…", "..."));
    }
    let prefix = icons.pick("\u{f07c} ", "📁 ", "");
    Some(format!("{prefix}{display}"))
}

fn push_git_counts(parts: &mut Vec<String>, st: super::super::GitStatusCounts, opts: Option<&GitSegmentOptions>) {
    if opts.and_then(|o| o.show_staged).unwrap_or(true) && st.staged > 0 {
        parts.push(format!("+{}", st.staged));
    }
    if opts.and_then(|o| o.show_unstaged).unwrap_or(true) && st.unstaged > 0 {
        parts.push(format!("*{}", st.unstaged));
    }
    if opts.and_then(|o| o.show_untracked).unwrap_or(true) && st.untracked > 0 {
        parts.push(format!("?{}", st.untracked));
    }
}

fn render_git_segment(
    footer: &FooterState,
    icons: StatusBarIconMode,
    opts: Option<&GitSegmentOptions>,
) -> Option<String> {
    let show_branch = opts.and_then(|o| o.show_branch).unwrap_or(true);
    let branch = footer.git_branch.as_deref().filter(|s| !s.is_empty());
    if branch.is_none() && footer.git_status.is_none() {
        return None;
    }

    let mut parts = Vec::new();
    if show_branch && let Some(b) = branch {
        let prefix = icons.pick("\u{e0a0} ", "⎇ ", "git:");
        parts.push(format!("{prefix}{}", sanitize_status_text(b)));
    }

    let is_dirty = footer.git_status.is_some_and(|st| {
        push_git_counts(&mut parts, st, opts);
        st.is_dirty()
    });

    if parts.is_empty() {
        return None;
    }
    let color = if is_dirty { "\x1b[33m" } else { "\x1b[32m" };
    Some(format!("{color}{}\x1b[39m", parts.join(" ")))
}

fn style_segment(id: StatusBarSegmentId, text: String, theme: &Theme) -> String {
    match id {
        StatusBarSegmentId::Pr | StatusBarSegmentId::Model => wrap_fg(&text, theme.prompt),
        StatusBarSegmentId::Path => wrap_fg(&text, theme.skill_tag),
        StatusBarSegmentId::Git | StatusBarSegmentId::ContextPct => text,
        StatusBarSegmentId::SessionName | StatusBarSegmentId::Subagents => wrap_fg(&text, theme.highlight),
        _ => wrap_fg(&text, theme.dimmed),
    }
}

pub(crate) fn context_threshold_color(pct: f64) -> &'static str {
    if pct >= 70.0 {
        "\x1b[31m"
    } else if pct >= 50.0 {
        "\x1b[33m"
    } else {
        "\x1b[32m"
    }
}

fn render_context_pct_segment(footer: &FooterState, icons: StatusBarIconMode) -> String {
    let pct = footer.context_percent.unwrap_or(0.0).clamp(0.0, 100.0);
    let color = context_threshold_color(pct);
    let prefix = icons.pick("\u{f0e7} ", "◔ ", "");
    if footer.context_window > 0 {
        format!(
            "{color}{prefix}{pct:.1}%/{}\x1b[39m",
            format_tokens(footer.context_window as u64)
        )
    } else {
        format!("{color}{prefix}{pct:.1}%\x1b[39m")
    }
}

fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn render_time_segment(opts: Option<&TimeSegmentOptions>, icons: StatusBarIconMode) -> String {
    let format = opts.and_then(|o| o.format).unwrap_or(TimeFormat::H24);
    let show_seconds = opts.and_then(|o| o.show_seconds).unwrap_or(false);
    let fmt = match (format, show_seconds) {
        (TimeFormat::H12, true) => "%I:%M:%S %p",
        (TimeFormat::H12, false) => "%I:%M %p",
        (TimeFormat::H24, true) => "%H:%M:%S",
        (TimeFormat::H24, false) => "%H:%M",
    };
    let prefix = icons.pick("\u{f017} ", "◷ ", "");
    format!("{prefix}{}", Local::now().format(fmt))
}

fn render_token_metric(id: StatusBarSegmentId, footer: &FooterState, ascii: bool) -> Option<String> {
    match id {
        StatusBarSegmentId::TokenIn => (footer.total_input_tokens > 0).then(|| {
            let p = if ascii { "in:" } else { "↑" };
            format!("{p}{}", format_tokens(footer.total_input_tokens))
        }),
        StatusBarSegmentId::TokenOut => (footer.total_output_tokens > 0).then(|| {
            let p = if ascii { "out:" } else { "↓" };
            format!("{p}{}", format_tokens(footer.total_output_tokens))
        }),
        StatusBarSegmentId::TokenTotal => {
            let total = footer.total_input_tokens.saturating_add(footer.total_output_tokens);
            (total > 0).then(|| {
                let p = if ascii { "tok:" } else { "Σ" };
                format!("{p}{}", format_tokens(total))
            })
        }
        StatusBarSegmentId::CacheRead => {
            (footer.total_cache_read_tokens > 0).then(|| format!("R{}", format_tokens(footer.total_cache_read_tokens)))
        }
        StatusBarSegmentId::CacheWrite => (footer.total_cache_write_tokens > 0)
            .then(|| format!("W{}", format_tokens(footer.total_cache_write_tokens))),
        StatusBarSegmentId::CacheHit => {
            let denom = footer.total_input_tokens.saturating_add(footer.total_cache_read_tokens);
            (denom > 0 && footer.total_cache_read_tokens > 0).then(|| {
                let pct = ((footer.total_cache_read_tokens as f64 / denom as f64) * 100.0).round() as u64;
                let p = if ascii { "hit:" } else { "⚡" };
                format!("{p}{pct}%")
            })
        }
        StatusBarSegmentId::TokenRate => footer
            .tokens_per_second
            .filter(|s| *s > 0.0)
            .map(|s| format!("@{}t/s", (s.round() as u64).max(1))),
        _ => None,
    }
}

fn render_meta_segment(id: StatusBarSegmentId, footer: &FooterState, icons: StatusBarIconMode) -> Option<String> {
    let ascii = matches!(icons, StatusBarIconMode::Ascii);
    match id {
        StatusBarSegmentId::ContextPct => Some(render_context_pct_segment(footer, icons)),
        StatusBarSegmentId::ContextTotal => {
            (footer.context_window > 0).then(|| format_tokens(footer.context_window as u64))
        }
        StatusBarSegmentId::Cost => footer.total_cost.filter(|c| *c > 0.0).map(|c| format!("${c:.3}")),
        StatusBarSegmentId::TimeSpent => (footer.active_duration_ms > 0).then(|| {
            let p = icons.pick("\u{f252} ", "⏱ ", "");
            format!("{p}{}", format_duration_ms(footer.active_duration_ms))
        }),
        StatusBarSegmentId::Session => footer
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| sanitize_status_text(s).chars().take(8).collect()),
        StatusBarSegmentId::SessionName => footer
            .session_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(sanitize_status_text),
        StatusBarSegmentId::Subagents => (footer.running_agents > 0).then(|| {
            if ascii {
                format!("agents:{}", footer.running_agents)
            } else {
                format!("⟳ {}", footer.running_agents)
            }
        }),
        StatusBarSegmentId::Quota => footer
            .quota
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(sanitize_status_text),
        StatusBarSegmentId::Hostname => footer
            .hostname
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(sanitize_status_text),
        _ => None,
    }
}

fn render_segment(
    id: StatusBarSegmentId,
    footer: &FooterState,
    resolved: &ResolvedStatusBar,
    theme: &Theme,
) -> Option<String> {
    let ascii = matches!(resolved.icons, StatusBarIconMode::Ascii);
    let raw = match id {
        StatusBarSegmentId::Pr => render_pr_segment(footer, resolved.icons),
        StatusBarSegmentId::Model => render_model_segment(
            footer,
            resolved.icons,
            resolved.compact_thinking_level,
            resolved.options.model.as_ref(),
        ),
        StatusBarSegmentId::Path => render_path_segment(footer, resolved.icons, resolved.options.path.as_ref()),
        StatusBarSegmentId::Git => render_git_segment(footer, resolved.icons, resolved.options.git.as_ref()),
        StatusBarSegmentId::Time => Some(render_time_segment(resolved.options.time.as_ref(), resolved.icons)),
        StatusBarSegmentId::TokenIn
        | StatusBarSegmentId::TokenOut
        | StatusBarSegmentId::TokenTotal
        | StatusBarSegmentId::CacheRead
        | StatusBarSegmentId::CacheWrite
        | StatusBarSegmentId::CacheHit
        | StatusBarSegmentId::TokenRate => render_token_metric(id, footer, ascii),
        _ => render_meta_segment(id, footer, resolved.icons),
    }?;
    Some(style_segment(id, raw, theme))
}
