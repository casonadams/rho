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
    let (prefix, label) = if fmt.is_selected {
        (
            format!("{highlight}▸{highlight:#} "),
            format!("{bold}{}{bold:#}", opt.label),
        )
    } else {
        ("  ".to_string(), opt.label.clone())
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
            String::new()
        } else {
            format!(" {dimmed}· default{dimmed:#}")
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

pub(crate) fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select if modal.title == "Select Model" => {
            "Enter to select • Ctrl+S to set as default • Esc to cancel"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Select Theme" => {
            "↑/↓ preview • Enter select • Esc cancel"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Conversation Tree" => {
            "↑/↓ select • Enter navigate • Shift+L label • Esc cancel"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Settings" => {
            "↑/↓ select • Enter toggle • Esc close"
        }
        crate::ui::interactive::ModalMode::Select if modal.title == "Resume Session" => {
            "↑/↓ select • Enter resume • Ctrl+D delete • Esc cancel"
        }
        crate::ui::interactive::ModalMode::Select if modal.is_searchable => "Enter to select • Esc to cancel",
        crate::ui::interactive::ModalMode::Select
            if modal.title.contains("Permission") || modal.title.contains("Approve") =>
        {
            "↑/↓ select • Enter confirm • Esc deny"
        }
        crate::ui::interactive::ModalMode::Select if modal.allow_custom => {
            "↑/↓ select • Enter confirm • Esc cancel • or type custom"
        }
        crate::ui::interactive::ModalMode::Select => "↑/↓ select • Enter confirm • Esc cancel",
        crate::ui::interactive::ModalMode::Input { .. } if modal.options.is_empty() => "Enter submit • Esc cancel",
        crate::ui::interactive::ModalMode::Input { .. } => "Enter submit • Esc back",
    }
}

pub(super) struct ModalOptionsLayout<'a> {
    pub inner_width: usize,
    pub max_visible: usize,
    pub theme: &'a crate::ui::theme::Theme,
}

pub(super) fn render_modal_options(modal: &ModalState, layout: ModalOptionsLayout<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    let dimmed = layout.theme.dimmed;
    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        lines.push(format!("    {dimmed}{msg}{dimmed:#}"));
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
        lines.push(format!("    {dimmed}({}/{}){dimmed:#}", modal.selected + 1, total));
    }

    if modal.title == "Select Model"
        && layout.max_visible >= 5
        && let Some(selected_opt) = modal.options.get(modal.selected)
        && let Some(extra) = selected_opt.description.as_deref().and_then(|d| d.split('\t').nth(3))
        && !extra.is_empty()
    {
        lines.push(String::new());
        lines.push(format!(
            "  {dimmed}Model Name: {} ({extra}){dimmed:#}",
            selected_opt.label
        ));
    }

    lines.truncate(layout.max_visible);
    lines
}
