use anstyle::{Color, Style};
use rho_harness_core::config::{StatusBarConfig, StatusBarSeparator};

use super::super::text::{sanitize_status_text, truncate_to_width};
use super::presets::resolve_status_bar;
use super::{ResolvedStatusBar, render_segment};
use crate::ui::block::visible_width;
use crate::ui::interactive::FooterState;
use crate::ui::theme::{SGR_RESET, Theme};

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

pub(super) fn wrap_fg(text: &str, style: Style) -> String {
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
        GroupSide::Left => format!("{bg} {body} \x1b[49m{cap_fg}\u{e0b0}{SGR_RESET}"),
        GroupSide::Right => format!("{cap_fg}\u{e0b2}{bg}\x1b[39m {body} {SGR_RESET}"),
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
    format!("{}{SGR_RESET}", segments.join(&sep_str))
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
