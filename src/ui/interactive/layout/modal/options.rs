use crate::ui::interactive::layout::text::wrap_to_width;
use crate::ui::interactive::{ModalOption, ModalState};

pub(super) fn format_option_line(opt: &ModalOption, is_selected: bool, is_model_selector: bool) -> String {
    let prefix = if is_selected { "\x1b[36m▸\x1b[0m " } else { "  " };
    let label = if is_selected {
        format!("\x1b[1m{}\x1b[0m", opt.label)
    } else {
        opt.label.clone()
    };
    let Some(desc) = &opt.description else {
        return format!("{prefix}{label}");
    };
    if is_model_selector && desc.contains('\t') {
        let mut p = desc.split('\t');
        let (prov, active, def) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
        let prov = if prov.is_empty() {
            String::new()
        } else {
            format!(" \x1b[2m[{prov}]\x1b[0m")
        };
        let def = if def.is_empty() {
            ""
        } else {
            " \x1b[2m· default\x1b[0m"
        };
        let check = if active.is_empty() { "" } else { " \x1b[32m✓\x1b[0m" };
        return format!("{prefix}{label}{prov}{def}{check}");
    }
    let cleaned_desc = desc.replace('\t', " • ");
    format!("{prefix}{label}  \x1b[2m{cleaned_desc}\x1b[0m")
}

pub(super) fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select if modal.title == "Select Model" => {
            "\x1b[2mEnter to select • Ctrl+S to set as default • Esc to cancel\x1b[0m"
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

pub(super) fn render_modal_options(modal: &ModalState, inner_width: usize, max_visible: usize) -> Vec<String> {
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
    let is_model_selector = modal.title == "Select Model";

    let (start, end, show_pagination) = if total <= max_visible {
        (0, total, false)
    } else {
        let page_size = max_visible.saturating_sub(1).max(1);
        let start = modal
            .selected
            .saturating_sub(page_size / 2)
            .min(total.saturating_sub(page_size));
        let end = (start + page_size).min(total);
        (start, end, true)
    };

    for i in start..end {
        let is_selected = i == modal.selected;
        let opt_line = format_option_line(&modal.options[i], is_selected, is_model_selector);
        for wrapped in wrap_to_width(&opt_line, inner_width) {
            lines.push(format!("  {wrapped}"));
        }
    }

    if show_pagination || start > 0 {
        lines.push(format!("    \x1b[2m({}/{})\x1b[0m", modal.selected + 1, total));
    }

    if is_model_selector
        && max_visible >= 5
        && let Some(selected_opt) = modal.options.get(modal.selected)
        && let Some(extra) = selected_opt.description.as_deref().and_then(|d| d.split('\t').nth(3))
        && !extra.is_empty()
    {
        lines.push(String::new());
        lines.push(format!("  \x1b[2mModel Name: {} ({extra})\x1b[0m", selected_opt.label));
    }

    lines.truncate(max_visible);
    lines
}
