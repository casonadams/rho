use crate::ui::interactive::layout::text::wrap_to_width;
use crate::ui::interactive::{ModalOption, ModalState, OptionLayout};

pub(super) struct OptionFormat<'a> {
    pub is_selected: bool,
    pub is_selector: bool,
    pub theme: &'a crate::ui::theme::Theme,
}

fn format_selector_tab_desc(desc: &str, theme: &crate::ui::theme::Theme) -> String {
    let mut p = desc.split('\t');
    let (prov, active, def) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
    let prov_s = if prov.is_empty() {
        String::new()
    } else {
        format!(" {}[{prov}]{}", theme.dimmed, theme.dimmed)
    };
    let def_s = if def.is_empty() {
        String::new()
    } else {
        format!(" {}· default{}", theme.dimmed, theme.dimmed)
    };
    let check_s = if active.is_empty() {
        String::new()
    } else {
        format!(" {}✓{}", theme.tool_ok, theme.tool_ok)
    };
    format!("{prov_s}{def_s}{check_s}")
}

pub(super) fn format_option_line(opt: &ModalOption, fmt: OptionFormat<'_>) -> String {
    let highlight = fmt.theme.highlight;
    let (prefix, label) = if fmt.is_selected {
        (
            format!("{highlight}▸{highlight:#} "),
            format!("\x1b[1m{}\x1b[0m", opt.label),
        )
    } else {
        ("  ".to_string(), opt.label.clone())
    };
    let Some(desc) = &opt.description else {
        return format!("{prefix}{label}");
    };
    if fmt.is_selector && desc.contains('\t') {
        let tabs = format_selector_tab_desc(desc, fmt.theme);
        return format!("{prefix}{label}{tabs}");
    }
    let cleaned = desc.replace('\t', " • ");
    format!("{prefix}{label}  {}{cleaned}{}", fmt.theme.dimmed, fmt.theme.dimmed)
}

const TITLE_HINTS: &[(&str, &str)] = &[
    (
        "Select Model",
        "Enter to select • Ctrl+S to set as default • Esc to cancel",
    ),
    ("Select Theme", "↑/↓ preview • Enter select • Esc cancel"),
    (
        "Conversation Tree",
        "↑/↓ select • Enter navigate • Shift+L label • Esc cancel",
    ),
    ("Settings", "↑/↓ select • Enter toggle • Esc close"),
    (
        "Resume Session",
        "↑/↓ select • Enter resume • Ctrl+D delete • Esc cancel",
    ),
];

fn fallback_select_hint(modal: &ModalState) -> &'static str {
    if modal.title.contains("Permission") || modal.title.contains("Approve") {
        "↑/↓ select • Enter confirm • Esc deny"
    } else if modal.allow_custom {
        "↑/↓ select • Enter confirm • Esc cancel • or type custom"
    } else {
        "↑/↓ select • Enter confirm • Esc cancel"
    }
}

fn select_modal_hint(modal: &ModalState) -> &'static str {
    if let Some((_, hint)) = TITLE_HINTS.iter().find(|(title, _)| modal.title == *title) {
        return hint;
    }
    if modal.is_searchable {
        "Enter to select • Esc to cancel"
    } else if modal.option_layout == OptionLayout::Horizontal {
        "←/→ or h/l select • ↑/↓ or j/k scroll • Enter confirm • Esc deny"
    } else {
        fallback_select_hint(modal)
    }
}

pub(crate) fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select => select_modal_hint(modal),
        crate::ui::interactive::ModalMode::Input { .. } if modal.options.is_empty() => "Enter submit • Esc cancel",
        crate::ui::interactive::ModalMode::Input { .. } => "Enter submit • Esc back",
    }
}

pub(crate) struct ModalOptionsLayout<'a> {
    pub inner_width: usize,
    pub max_visible: usize,
    pub theme: &'a crate::ui::theme::Theme,
}

fn calculate_pagination(total: usize, selected: usize, max_visible: usize) -> (usize, usize, bool) {
    if total <= max_visible {
        (0, total, false)
    } else {
        let page_size = max_visible.saturating_sub(1).max(1);
        let start = selected
            .saturating_sub(page_size / 2)
            .min(total.saturating_sub(page_size));
        let end = (start + page_size).min(total);
        (start, end, true)
    }
}

fn push_model_extra(modal: &ModalState, (max_visible, dimmed): (usize, anstyle::Style), lines: &mut Vec<String>) {
    if modal.title != "Select Model" || max_visible < 5 {
        return;
    }
    if let Some(opt) = modal.options.get(modal.selected)
        && let Some(extra) = opt.description.as_deref().and_then(|d| d.split('\t').nth(3))
        && !extra.is_empty()
    {
        lines.push(String::new());
        lines.push(format!("  {dimmed}Model Name: {} ({extra}){dimmed:#}", opt.label));
    }
}

fn render_visible_options(
    modal: &ModalState,
    layout: &ModalOptionsLayout<'_>,
    (start, end): (usize, usize),
) -> Vec<String> {
    let mut lines = Vec::new();
    let is_selector = modal.title == "Select Model" || modal.title == "Select Theme";
    for i in start..end {
        let opt_line = format_option_line(
            &modal.options[i],
            OptionFormat {
                is_selected: i == modal.selected,
                is_selector,
                theme: layout.theme,
            },
        );
        for wrapped in wrap_to_width(&opt_line, layout.inner_width) {
            lines.push(format!("  {wrapped}"));
        }
    }
    lines
}

pub(crate) fn render_modal_options(modal: &ModalState, layout: ModalOptionsLayout<'_>) -> Vec<String> {
    if modal.option_layout == OptionLayout::Horizontal {
        return super::horizontal::render_horizontal_options(modal, &layout);
    }
    let dimmed = layout.theme.dimmed;
    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        return vec![format!("    {dimmed}{msg}{dimmed:#}")];
    }
    let total = modal.options.len();
    let (start, end, show_pagination) = calculate_pagination(total, modal.selected, layout.max_visible);
    let mut lines = render_visible_options(modal, &layout, (start, end));
    if show_pagination || start > 0 {
        lines.push(format!("    {dimmed}({}/{}){dimmed:#}", modal.selected + 1, total));
    }
    push_model_extra(modal, (layout.max_visible, dimmed), &mut lines);
    lines.truncate(layout.max_visible);
    lines
}
