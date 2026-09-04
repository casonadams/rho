use crate::ui::interactive::layout::text::wrap_to_width;
use crate::ui::interactive::{ModalOption, ModalState};

pub(super) struct OptionFormat<'a> {
    pub is_selected: bool,
    pub is_selector: bool,
    pub theme: &'a crate::ui::theme::Theme,
}

pub(super) fn format_option_line(opt: &ModalOption, fmt: OptionFormat<'_>) -> String {
    let highlight = fmt.theme.highlight;
    let tool_ok = fmt.theme.tool_ok;
    let dimmed = fmt.theme.dimmed;
    let bold = anstyle::Style::new().bold();
    let prefix = if fmt.is_selected {
        format!("{highlight}▸{highlight:#} ")
    } else {
        "  ".to_string()
    };
    let label = if fmt.is_selected {
        format!("{bold}{}{bold:#}", opt.label)
    } else {
        opt.label.clone()
    };
    let Some(desc) = &opt.description else {
        return format!("{prefix}{label}");
    };
    if fmt.is_selector && desc.contains('\t') {
        let mut p = desc.split('\t');
        let (prov, active, def) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
        let prov = if prov.is_empty() {
            String::new()
        } else {
            format!(" {dimmed}[{prov}]{dimmed:#}")
        };
        let def = if def.is_empty() {
            ""
        } else {
            " \x1b[2m· default\x1b[0m"
        };
        let check = if active.is_empty() {
            String::new()
        } else {
            format!(" {tool_ok}✓{tool_ok:#}")
        };
        return format!("{prefix}{label}{prov}{def}{check}");
    }
    let cleaned_desc = desc.replace('\t', " • ");
    format!("{prefix}{label}  {dimmed}{cleaned_desc}{dimmed:#}")
}

pub(super) fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select if modal.title == "Select Model" => {
            "\x1b[2mEnter to select • Ctrl+S to set as default • Esc to cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Select Theme" => {
            "\x1b[2m↑/↓ preview • Enter select • Esc cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Conversation Tree" => {
            "\x1b[2m↑/↓ select • Enter navigate • Shift+L label • Esc cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Settings" => {
            "\x1b[2m↑/↓ select • Enter toggle • Esc close\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Resume Session" => {
            "\x1b[2m↑/↓ select • Enter resume • Ctrl+D delete • Esc cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.is_searchable => {
            "\x1b[2mEnter to select • Esc to cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select
            if modal.title.contains("Permission") || modal.title.contains("Approve") =>
        {
            "\x1b[2m↑/↓ select • Enter confirm • Esc deny\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select if modal.allow_custom => {
            "\x1b[2m↑/↓ select • Enter confirm • Esc cancel • or type custom\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Select => "\x1b[2m↑/↓ select • Enter confirm • Esc cancel\x1b[0m",
        crate::ui::interactive::ModalMode::Input { .. } if modal.options.is_empty() => {
            "\x1b[2mEnter submit • Esc cancel\x1b[0m"
        }
        crate::ui::interactive::ModalMode::Input { .. } => "\x1b[2mEnter submit • Esc back\x1b[0m",
    }
}

pub(super) struct ModalOptionsLayout<'a> {
    pub inner_width: usize,
    pub max_visible: usize,
    pub theme: &'a crate::ui::theme::Theme,
}

pub(super) fn render_modal_options(modal: &ModalState, layout: ModalOptionsLayout<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        lines.push(format!("    \x1b[2m{msg}\x1b[0m"));
        return lines;
    }

    let total = modal.options.len();
    let is_selector = modal.title == "Select Model" || modal.title == "Select Theme";

    let (start, end, show_pagination) = if total <= layout.max_visible {
        (0, total, false)
    } else {
        let page_size = layout.max_visible.saturating_sub(1).max(1);
        let start = modal
            .selected
            .saturating_sub(page_size / 2)
            .min(total.saturating_sub(page_size));
        let end = (start + page_size).min(total);
        (start, end, true)
    };

    for i in start..end {
        let is_selected = i == modal.selected;
        let opt_line = format_option_line(
            &modal.options[i],
            OptionFormat {
                is_selected,
                is_selector,
                theme: layout.theme,
            },
        );
        for wrapped in wrap_to_width(&opt_line, layout.inner_width) {
            lines.push(format!("  {wrapped}"));
        }
    }

    if show_pagination || start > 0 {
        lines.push(format!("    \x1b[2m({}/{})\x1b[0m", modal.selected + 1, total));
    }

    if modal.title == "Select Model"
        && layout.max_visible >= 5
        && let Some(selected_opt) = modal.options.get(modal.selected)
        && let Some(extra) = selected_opt.description.as_deref().and_then(|d| d.split('\t').nth(3))
        && !extra.is_empty()
    {
        lines.push(String::new());
        lines.push(format!("  \x1b[2mModel Name: {} ({extra})\x1b[0m", selected_opt.label));
    }

    lines.truncate(layout.max_visible);
    lines
}
