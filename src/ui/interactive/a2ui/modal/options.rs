use rho_harness_core::a2ui::{A2uiSurface, ChoiceOption};

use crate::ui::block::visible_width;
use crate::ui::interactive::footer::truncate_with_marker;
use crate::ui::theme::{CursorMode, SGR_RESET, Theme};

const CURSOR_BLOCK: &str = "\x1b[7m \x1b[27m";

pub(super) fn option_prefix(theme: &Theme, active: bool) -> String {
    if active {
        format!("{}▸{:#} ", theme.highlight, theme.highlight)
    } else {
        "  ".to_string()
    }
}

#[derive(Clone, Copy)]
pub(super) enum ChoiceMark {
    Radio(bool),
    Checkbox(bool),
}

impl ChoiceMark {
    fn is_checked(self) -> bool {
        matches!(self, Self::Radio(true) | Self::Checkbox(true))
    }
}

pub(super) fn check_indicator(theme: &Theme, mark: ChoiceMark) -> String {
    match mark {
        ChoiceMark::Checkbox(true) => format!("{}[x]{:#}", theme.tool_ok, theme.tool_ok),
        ChoiceMark::Checkbox(false) => "[ ]".to_string(),
        ChoiceMark::Radio(true) => format!("{}●{:#}", theme.highlight, theme.highlight),
        ChoiceMark::Radio(false) => "○".to_string(),
    }
}

pub(super) fn software_cursor(theme: &Theme, active: bool) -> &'static str {
    if active && theme.cursor_mode == CursorMode::Software {
        CURSOR_BLOCK
    } else {
        ""
    }
}

pub(super) struct ChoiceOptionFormat<'a> {
    pub surface: &'a A2uiSurface,
    pub opt: &'a ChoiceOption,
    pub active_cursor: bool,
    pub mark: ChoiceMark,
    pub label_col_w: usize,
    pub max_width: usize,
    pub theme: &'a Theme,
}

pub(super) fn format_choice_option_line(spec: ChoiceOptionFormat<'_>) -> String {
    let ChoiceOptionFormat {
        surface,
        opt,
        active_cursor,
        mark,
        label_col_w,
        max_width,
        theme,
    } = spec;
    let prefix = option_prefix(theme, active_cursor);
    let indicator = check_indicator(theme, mark);
    let label = surface.resolve_string(&opt.label);
    let label_w = visible_width(&label);
    let styled_label = if active_cursor {
        format!("\x1b[1m{label}{SGR_RESET}")
    } else {
        label
    };
    let Some(desc) = &opt.description else {
        return format!("{prefix}{indicator} {styled_label}");
    };
    let pad = label_col_w.saturating_sub(label_w);
    let used_w = visible_width(&prefix) + visible_width(&indicator) + 1 + label_w + pad + 2;
    let desc_max = max_width.saturating_sub(used_w);
    if desc_max == 0 {
        return format!("{prefix}{indicator} {styled_label}");
    }
    let d = surface.resolve_string(desc);
    let truncated_desc = truncate_description(&d, desc_max);
    format!(
        "{prefix}{indicator} {styled_label}{}  {}{truncated_desc}{:#}",
        " ".repeat(pad),
        theme.dimmed,
        theme.dimmed
    )
}

fn truncate_description(desc: &str, max_w: usize) -> String {
    if visible_width(desc) <= max_w {
        return desc.to_string();
    }
    if max_w <= 1 {
        return "…".to_string();
    }
    truncate_with_marker(desc, max_w, "…")
}

pub(super) struct OtherOptionRender<'a> {
    pub active_cursor: bool,
    pub mark: ChoiceMark,
    pub other_text: &'a str,
    pub editing: bool,
    pub focused: bool,
    pub theme: &'a Theme,
}

pub(super) fn format_other_option_line(spec: OtherOptionRender<'_>) -> (String, usize) {
    let OtherOptionRender {
        active_cursor,
        mark,
        other_text,
        editing,
        focused,
        theme,
    } = spec;
    let prefix = option_prefix(theme, active_cursor);
    let indicator = check_indicator(theme, mark);
    if !mark.is_checked() && !editing && other_text.is_empty() {
        let label = if active_cursor {
            format!("\x1b[1mOther…{SGR_RESET}")
        } else {
            "Other…".to_string()
        };
        return (format!("{prefix}{indicator} {label}"), 0);
    }
    let base = format!("{prefix}{indicator} Other: [");
    let col = visible_width(&base) + visible_width(other_text);
    let cursor_block = software_cursor(theme, editing && focused);
    (format!("{base}{other_text}{cursor_block}]"), col)
}
