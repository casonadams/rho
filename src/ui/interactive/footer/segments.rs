use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use anstyle::{Color, Style};
use chrono::Local;
use rho_harness_core::config::{
    ContextLineMode, GitSegmentOptions, ModelSegmentOptions, PathSegmentOptions, StatusBarConfig, StatusBarIconMode,
    StatusBarPreset, StatusBarSegmentId, StatusBarSegmentOptions, StatusBarSeparator, TimeFormat, TimeSegmentOptions,
};
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::thinking::ThinkingLevel;

use super::super::FooterState;
use super::path::abbreviate_home;
use super::text::{format_tokens, sanitize_status_text, truncate_to_width};
use crate::ui::block::visible_width;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupSide {
    Left,
    Right,
}

fn fg_code(style: Style) -> String {
    let Some(fg) = style.get_fg_color() else {
        return String::new();
    };
    format!("{}", Style::new().fg_color(Some(fg)))
}

fn wrap_fg(text: &str, style: Style) -> String {
    let fg = fg_code(style);
    if fg.is_empty() {
        text.to_string()
    } else {
        format!("{fg}{text}\x1b[39m")
    }
}

fn surface_codes(theme: Option<&Theme>) -> Option<(String, String)> {
    let bg: Color = theme?.block_fill.get_bg_color()?;
    let bg_ansi = format!("{}", Style::new().bg_color(Some(bg)));
    let cap_fg_ansi = format!("{}", Style::new().fg_color(Some(bg)));
    Some((bg_ansi, cap_fg_ansi))
}

const DEFAULT_LEFT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::Model,
    StatusBarSegmentId::Path,
    StatusBarSegmentId::Git,
];
const DEFAULT_RIGHT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::TokenTotal,
    StatusBarSegmentId::Cost,
    StatusBarSegmentId::ContextPct,
    StatusBarSegmentId::Quota,
];
const MINIMAL_LEFT: &[StatusBarSegmentId] = &[StatusBarSegmentId::Path, StatusBarSegmentId::Git];
const MINIMAL_RIGHT: &[StatusBarSegmentId] = &[StatusBarSegmentId::SessionName, StatusBarSegmentId::ContextPct];
const COMPACT_LEFT: &[StatusBarSegmentId] = &[StatusBarSegmentId::Model, StatusBarSegmentId::Git];
const COMPACT_RIGHT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::SessionName,
    StatusBarSegmentId::Cost,
    StatusBarSegmentId::ContextPct,
];
const FULL_LEFT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::Hostname,
    StatusBarSegmentId::Model,
    StatusBarSegmentId::Path,
    StatusBarSegmentId::Git,
    StatusBarSegmentId::Subagents,
];
const FULL_RIGHT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::SessionName,
    StatusBarSegmentId::CacheHit,
    StatusBarSegmentId::TokenIn,
    StatusBarSegmentId::TokenOut,
    StatusBarSegmentId::TokenRate,
    StatusBarSegmentId::CacheRead,
    StatusBarSegmentId::Cost,
    StatusBarSegmentId::ContextPct,
    StatusBarSegmentId::TimeSpent,
    StatusBarSegmentId::Time,
];
const NERD_LEFT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::Hostname,
    StatusBarSegmentId::Model,
    StatusBarSegmentId::Path,
    StatusBarSegmentId::Git,
    StatusBarSegmentId::Session,
    StatusBarSegmentId::Subagents,
];
const NERD_RIGHT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::SessionName,
    StatusBarSegmentId::TokenIn,
    StatusBarSegmentId::TokenOut,
    StatusBarSegmentId::CacheRead,
    StatusBarSegmentId::CacheWrite,
    StatusBarSegmentId::TokenRate,
    StatusBarSegmentId::Cost,
    StatusBarSegmentId::ContextPct,
    StatusBarSegmentId::ContextTotal,
    StatusBarSegmentId::TimeSpent,
    StatusBarSegmentId::Time,
];
const ASCII_LEFT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::Model,
    StatusBarSegmentId::Path,
    StatusBarSegmentId::Git,
];
const ASCII_RIGHT: &[StatusBarSegmentId] = &[
    StatusBarSegmentId::SessionName,
    StatusBarSegmentId::TokenTotal,
    StatusBarSegmentId::Cost,
    StatusBarSegmentId::ContextPct,
];

pub(crate) fn is_default_status_bar(cfg: &StatusBarConfig) -> bool {
    matches!(cfg.preset, None | Some(StatusBarPreset::Default))
        && cfg.left.is_none()
        && cfg.right.is_none()
        && cfg.separator.is_none()
        && cfg.icons.is_none()
        && cfg.transparent.is_none()
        && cfg.compact_thinking_level.is_none()
        && cfg.context_line.is_none()
        && cfg.segment_options.is_none()
}

pub fn status_bar_uses_segment(cfg: &StatusBarConfig, target: StatusBarSegmentId) -> bool {
    if is_default_status_bar(cfg) {
        return false;
    }
    let resolved = resolve_status_bar(cfg);
    resolved.left.contains(&target) || resolved.right.contains(&target)
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

fn preset_defaults(
    preset: StatusBarPreset,
) -> (
    &'static [StatusBarSegmentId],
    &'static [StatusBarSegmentId],
    StatusBarSeparator,
    StatusBarIconMode,
) {
    match preset {
        StatusBarPreset::Minimal => (
            MINIMAL_LEFT,
            MINIMAL_RIGHT,
            StatusBarSeparator::Slash,
            StatusBarIconMode::Unicode,
        ),
        StatusBarPreset::Compact => (
            COMPACT_LEFT,
            COMPACT_RIGHT,
            StatusBarSeparator::PowerlineThin,
            StatusBarIconMode::Unicode,
        ),
        StatusBarPreset::Full => (
            FULL_LEFT,
            FULL_RIGHT,
            StatusBarSeparator::Powerline,
            StatusBarIconMode::Unicode,
        ),
        StatusBarPreset::Nerd => (
            NERD_LEFT,
            NERD_RIGHT,
            StatusBarSeparator::Powerline,
            StatusBarIconMode::Nerd,
        ),
        StatusBarPreset::Ascii => (
            ASCII_LEFT,
            ASCII_RIGHT,
            StatusBarSeparator::Ascii,
            StatusBarIconMode::Ascii,
        ),
        StatusBarPreset::Default | StatusBarPreset::Custom => (
            DEFAULT_LEFT,
            DEFAULT_RIGHT,
            StatusBarSeparator::Powerline,
            StatusBarIconMode::Unicode,
        ),
    }
}

fn resolve_status_bar(cfg: &StatusBarConfig) -> ResolvedStatusBar {
    let preset = cfg.preset.unwrap_or(StatusBarPreset::Default);
    let (def_left, def_right, def_sep, def_icons) = preset_defaults(preset);
    let absorb_context = matches!(cfg.context_line, Some(ContextLineMode::Embedded));
    let filter_ctx = |list: Vec<StatusBarSegmentId>| -> Vec<StatusBarSegmentId> {
        if absorb_context {
            list.into_iter()
                .filter(|id| !matches!(id, StatusBarSegmentId::ContextPct | StatusBarSegmentId::ContextTotal))
                .collect()
        } else {
            list
        }
    };
    ResolvedStatusBar {
        left: filter_ctx(cfg.left.clone().unwrap_or_else(|| def_left.to_vec())),
        right: filter_ctx(cfg.right.clone().unwrap_or_else(|| def_right.to_vec())),
        separator: cfg.separator.unwrap_or(def_sep),
        icons: cfg.icons.unwrap_or(def_icons),
        transparent: cfg.transparent.unwrap_or(false),
        compact_thinking_level: cfg.compact_thinking_level.unwrap_or(false),
        options: cfg.segment_options.clone().unwrap_or_default(),
    }
}

fn compact_level_icon(level: ThinkingLevel) -> &'static str {
    match level {
        ThinkingLevel::Off => "○",
        ThinkingLevel::Minimal => "◔",
        ThinkingLevel::Low => "◑",
        ThinkingLevel::Medium => "◒",
        ThinkingLevel::High => "◕",
        ThinkingLevel::XHigh | ThinkingLevel::Max => "◉",
        ThinkingLevel::Auto => "◐",
    }
}

fn compact_level_name(level: ThinkingLevel) -> &'static str {
    match level {
        ThinkingLevel::Off => "off",
        ThinkingLevel::Minimal => "min",
        ThinkingLevel::Low => "low",
        ThinkingLevel::Medium => "med",
        ThinkingLevel::High => "high",
        ThinkingLevel::XHigh => "xhigh",
        ThinkingLevel::Max => "max",
        ThinkingLevel::Auto => "auto",
    }
}

fn compact_thinking_label(level: Option<&str>, resolved: Option<ThinkingLevel>) -> (&'static str, &'static str) {
    match (ThinkingLevel::from_setting(level), resolved) {
        (ThinkingLevel::Auto, Some(r)) => ("◐", compact_level_name(r)),
        (l, _) => (compact_level_icon(l), compact_level_name(l)),
    }
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
    let prefix = match icons {
        StatusBarIconMode::Nerd => "\u{f06a9} ",
        StatusBarIconMode::Unicode => "◆ ",
        StatusBarIconMode::Ascii => "",
    };
    let name = sanitize_status_text(&model_name_part(footer, short_name, opts));
    if !show_thinking {
        return Some(format!("{prefix}{name}"));
    }
    let (icon, level) = compact_thinking_label(footer.thinking_level.as_deref(), footer.resolved_thinking_level);
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
    let label = match icons {
        StatusBarIconMode::Nerd => format!("\u{f407} #{}", pr.number),
        StatusBarIconMode::Unicode | StatusBarIconMode::Ascii => format!("PR #{}", pr.number),
    };
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

fn truncate_path_left(text: &str, max_len: usize, ascii: bool) -> String {
    if text.width() <= max_len {
        return text.to_string();
    }
    let ellipsis = if ascii { "..." } else { "…" };
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
        display = truncate_path_left(&display, max_len, matches!(icons, StatusBarIconMode::Ascii));
    }
    let prefix = match icons {
        StatusBarIconMode::Nerd => "\u{f07c} ",
        StatusBarIconMode::Unicode => "📁 ",
        StatusBarIconMode::Ascii => "",
    };
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
        let prefix = match icons {
            StatusBarIconMode::Nerd => "\u{e0a0} ",
            StatusBarIconMode::Unicode => "⎇ ",
            StatusBarIconMode::Ascii => "git:",
        };
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
    let prefix = match icons {
        StatusBarIconMode::Nerd => "\u{f0e7} ",
        StatusBarIconMode::Unicode => "◔ ",
        StatusBarIconMode::Ascii => "",
    };
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
    let prefix = match icons {
        StatusBarIconMode::Nerd => "\u{f017} ",
        StatusBarIconMode::Unicode => "◷ ",
        StatusBarIconMode::Ascii => "",
    };
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
            let p = match icons {
                StatusBarIconMode::Nerd => "\u{f252} ",
                StatusBarIconMode::Unicode => "⏱ ",
                StatusBarIconMode::Ascii => "",
            };
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

fn plain_separator(sep: StatusBarSeparator, side: GroupSide) -> &'static str {
    match sep {
        StatusBarSeparator::Powerline | StatusBarSeparator::PowerlineThin => match side {
            GroupSide::Left => " \u{e0b1} ",
            GroupSide::Right => " \u{e0b3} ",
        },
        StatusBarSeparator::Dot => " · ",
        StatusBarSeparator::Slash => " / ",
        StatusBarSeparator::Pipe => " │ ",
        StatusBarSeparator::Block => " █ ",
        StatusBarSeparator::None => " ",
        StatusBarSeparator::Ascii => match side {
            GroupSide::Left => " > ",
            GroupSide::Right => " < ",
        },
    }
}

fn join_surface_group(
    segments: &[String],
    sep: StatusBarSeparator,
    side: GroupSide,
    surface: (&str, &str),
    dim: (&str, &str),
) -> String {
    let (bg, cap_fg) = surface;
    let (dim_start, dim_end) = dim;
    let inner_sep = format!("{dim_start}{}{dim_end}", plain_separator(sep, side));
    let body = segments.join(&inner_sep);
    match side {
        GroupSide::Left => format!("{bg} {body} \x1b[49m{cap_fg}\u{e0b0}\x1b[0m"),
        GroupSide::Right => format!("{cap_fg}\u{e0b2}{bg}\x1b[39m {body} \x1b[0m"),
    }
}

fn join_group(
    segments: &[String],
    sep: StatusBarSeparator,
    side: GroupSide,
    transparent: bool,
    theme: Option<&Theme>,
) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let (dim_start, dim_end) = match theme {
        Some(t) => (fg_code(t.dimmed), "\x1b[39m"),
        None => ("\x1b[2m".to_string(), "\x1b[22m"),
    };
    if !transparent
        && matches!(sep, StatusBarSeparator::Powerline)
        && let Some((bg, cap_fg)) = surface_codes(theme)
    {
        return join_surface_group(segments, sep, side, (&bg, &cap_fg), (&dim_start, dim_end));
    }
    let sep_str = format!("{dim_start}{}{dim_end}", plain_separator(sep, side));
    format!("{}\x1b[0m", segments.join(&sep_str))
}

fn fit_segments(
    mut left: Vec<String>,
    mut right: Vec<String>,
    width: usize,
    resolved: &ResolvedStatusBar,
    theme: Option<&Theme>,
) -> String {
    let orig_right = right.clone();
    loop {
        let l = join_group(&left, resolved.separator, GroupSide::Left, resolved.transparent, theme);
        let r = join_group(
            &right,
            resolved.separator,
            GroupSide::Right,
            resolved.transparent,
            theme,
        );
        let lw = visible_width(&l);
        let rw = visible_width(&r);
        let min_gap = usize::from(!left.is_empty() && !right.is_empty());
        if lw + rw + min_gap <= width {
            let pad = width.saturating_sub(lw + rw);
            return format!("{l}{}{r}", " ".repeat(pad));
        }
        if !right.is_empty() {
            right.pop();
        } else if !left.is_empty() {
            left.pop();
            right = orig_right.clone();
        } else {
            return String::new();
        }
    }
}

pub(crate) fn format_segmented_status_bar(
    footer: &FooterState,
    cfg: &StatusBarConfig,
    width: usize,
    system_message: Option<&str>,
    theme: Option<&Theme>,
) -> Vec<String> {
    let default_theme = Theme::default();
    let active_theme = theme.unwrap_or(&default_theme);
    let resolved = resolve_status_bar(cfg);
    let left: Vec<String> = resolved
        .left
        .iter()
        .filter_map(|id| render_segment(*id, footer, &resolved, active_theme))
        .collect();
    let right: Vec<String> = resolved
        .right
        .iter()
        .filter_map(|id| render_segment(*id, footer, &resolved, active_theme))
        .collect();

    let bar = fit_segments(left, right, width, &resolved, theme);
    let mut lines = vec![bar];
    if let Some(msg) = system_message.filter(|s| !s.trim().is_empty()) {
        lines.push(truncate_to_width(&sanitize_status_text(msg), width));
    }
    lines
}
