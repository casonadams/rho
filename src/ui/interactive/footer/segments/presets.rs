use rho_harness_core::config::{
    ContextLineMode, StatusBarConfig, StatusBarIconMode, StatusBarPreset, StatusBarSegmentId, StatusBarSeparator,
};

use super::ResolvedStatusBar;

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

pub(super) fn resolve_status_bar(cfg: &StatusBarConfig) -> ResolvedStatusBar {
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
