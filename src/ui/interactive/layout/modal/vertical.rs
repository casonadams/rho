use super::horizontal::render_horizontal_options;
use super::types::{ModalOptionsLayout, OptionFormat};
use crate::ui::interactive::layout::text::wrap_to_width;
use crate::ui::interactive::{ModalOption, ModalState, OptionLayout};

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

pub fn format_option_line(opt: &ModalOption, fmt: OptionFormat<'_>) -> String {
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
    (
        "Select Thinking Level",
        "Enter to select • Ctrl+S to set as default • Esc to cancel",
    ),
    ("Select Theme", "↑/↓ preview • Enter select • Esc cancel"),
    (
        "Conversation Tree",
        "↑/↓ select • Enter navigate • Shift+L label • Esc cancel",
    ),
    ("Settings", "↑/↓ select • Enter toggle/open • Esc close"),
    ("Models", "↑/↓ select • Enter choose • Esc back"),
    ("Tools & Permissions", "↑/↓ select • Enter toggle/choose • Esc back"),
    (
        "Resume Session",
        "↑/↓ select • Enter resume • Ctrl+D delete • Esc cancel",
    ),
    (
        "Side Question (/btw)",
        "Enter send • Up/Down scroll • Ctrl+Y copy • Esc close",
    ),
];

/// Digits jump to an option only when the modal does not treat typed characters as text.
fn digits_jump(modal: &ModalState) -> bool {
    !modal.allow_custom && !modal.is_searchable
}

fn fallback_select_hint(modal: &ModalState) -> &'static str {
    let permission = modal.title.contains("Permission") || modal.title.contains("Approve");
    match (permission, modal.allow_custom) {
        (true, false) => "↑/↓ select • 1-9 jump • Enter confirm • Esc deny",
        (true, true) => "↑/↓ select • Enter confirm • Esc deny",
        (false, true) => "↑/↓ select • Enter confirm • Esc cancel • or type custom",
        (false, false) => "↑/↓ select • 1-9 jump • Enter confirm • Esc cancel",
    }
}

fn select_modal_hint(modal: &ModalState) -> &'static str {
    if modal.pending_delete.is_some() {
        return "Press Ctrl+D again to delete this session • any other key cancels";
    }
    if let Some((_, hint)) = TITLE_HINTS.iter().find(|(title, _)| modal.title == *title) {
        return hint;
    }
    if modal.is_searchable {
        "Enter to select • Esc to cancel"
    } else if modal.option_layout == OptionLayout::Horizontal {
        if digits_jump(modal) {
            "←/→ or h/l select • 1-9 jump • ↑/↓ or j/k scroll • Enter confirm • Esc deny"
        } else {
            "←/→ or h/l select • ↑/↓ or j/k scroll • Enter confirm • Esc deny"
        }
    } else {
        fallback_select_hint(modal)
    }
}

pub fn modal_hint(modal: &ModalState) -> &'static str {
    if let Some(a2ui) = &modal.a2ui {
        return a2ui.footer_hint();
    }
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select => select_modal_hint(modal),
        crate::ui::interactive::ModalMode::Input { .. } if modal.options.is_empty() => {
            if let Some((_, hint)) = TITLE_HINTS.iter().find(|(title, _)| modal.title == *title) {
                return hint;
            }
            "Enter submit • Shift+Enter newline • Esc cancel"
        }
        crate::ui::interactive::ModalMode::Input { .. } => "Enter submit • Shift+Enter newline • Esc back",
    }
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

fn render_visible_options(
    modal: &ModalState,
    layout: &ModalOptionsLayout<'_>,
    start: usize,
    end: usize,
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

pub fn render_modal_options(modal: &ModalState, layout: ModalOptionsLayout<'_>) -> Vec<String> {
    if modal.option_layout == OptionLayout::Horizontal {
        return render_horizontal_options(modal, &layout);
    }
    let total = modal.options.len();
    if total == 0 {
        if modal.is_searchable {
            let msg = if modal.title == "Select Model" {
                "No matching models found"
            } else {
                "No matching options found"
            };
            let dimmed = layout.theme.dimmed;
            return vec![format!("    {dimmed}{msg}{dimmed:#}")];
        }
        return Vec::new();
    }
    let max_visible = layout.max_visible.max(1);
    let (start, end, show_indicator) = calculate_pagination(total, modal.selected, max_visible);
    let mut lines = render_visible_options(modal, &layout, start, end);
    if show_indicator {
        let dimmed = layout.theme.dimmed;
        lines.push(format!("    {dimmed}••• ({total} options){dimmed:#}"));
    }
    lines
}
