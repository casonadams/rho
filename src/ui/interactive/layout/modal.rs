use super::text::{visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalState};

pub(crate) fn render_modal_overlay(modal: &ModalState, width: usize) -> (Vec<String>, CursorPosition, bool) {
    let width = width.max(20);
    let inner_width = width.saturating_sub(4).max(1);
    let mut lines = Vec::new();
    let mut cursor = CursorPosition { row: 0, column: 0 };
    let mut cursor_visible = false;

    lines.push("─".repeat(width));
    lines.push(format!("  \x1b[1;36m{}\x1b[0m", modal.title.trim()));

    if modal.is_searchable {
        let prefix = "  \x1b[1m>\x1b[0m ";
        let filter = &modal.filter_query;
        lines.push(format!("{prefix}{filter}"));
        cursor = CursorPosition {
            row: lines.len() - 1,
            column: visible_width(prefix) + visible_width(filter),
        };
        cursor_visible = true;
    }

    if !modal.body.trim().is_empty() {
        lines.push(String::new());
        for line in wrap_to_width(&modal.body, inner_width) {
            lines.push(format!("  {line}"));
        }
    }
    lines.push(String::new());

    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        lines.push(format!("    \x1b[2m{msg}\x1b[0m"));
    } else {
        let max_visible = 10;
        let total = modal.options.len();
        let start = if total <= max_visible {
            0
        } else {
            modal.selected.saturating_sub(max_visible / 2).min(total - max_visible)
        };
        let end = (start + max_visible).min(total);

        let is_model_selector = modal.title == "Select Model";
        for i in start..end {
            let is_selected = i == modal.selected;
            let opt_line = format_option_line(&modal.options[i], is_selected, is_model_selector);
            for wrapped in wrap_to_width(&opt_line, inner_width) {
                lines.push(format!("  {wrapped}"));
            }
        }

        if total > max_visible || start > 0 {
            lines.push(format!("    \x1b[2m({}/{})\x1b[0m", modal.selected + 1, total));
        }

        if is_model_selector
            && let Some(selected_opt) = modal.options.get(modal.selected)
            && let Some(extra) = selected_opt.description.as_deref().and_then(|d| d.split('\t').nth(3))
            && !extra.is_empty()
        {
            lines.push(String::new());
            lines.push(format!("  \x1b[2mModel Name: {} ({extra})\x1b[0m", selected_opt.label));
        }
    }

    if let ModalMode::Input { prompt_label } = &modal.mode {
        lines.push(String::new());
        let prefix = format!("\x1b[1;36m{prompt_label}:\x1b[0m ");
        let input_text = modal.input.text();
        let prompt_line = format!("{prefix}{input_text}");
        cursor = CursorPosition {
            row: lines.len(),
            column: 2 + visible_width(&prefix) + visible_width(&input_text[..modal.input.cursor()]),
        };
        cursor_visible = true;
        for wrapped in wrap_to_width(&prompt_line, inner_width) {
            lines.push(format!("  {wrapped}"));
        }
    }

    lines.push(String::new());
    lines.push(format!("  {}", modal_hint(modal)));
    lines.push("─".repeat(width));

    (lines, cursor, cursor_visible)
}

fn format_option_line(opt: &crate::ui::interactive::ModalOption, is_selected: bool, is_model_selector: bool) -> String {
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

fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        ModalMode::Select if modal.title == "Select Model" => {
            "\x1b[2mEnter to select • Ctrl+S to set as default • Esc to cancel\x1b[0m"
        }
        ModalMode::Select if modal.title == "Conversation Tree" => {
            "\x1b[2m↑/↓ select • Enter navigate • Shift+L label • Esc cancel\x1b[0m"
        }
        ModalMode::Select if modal.title == "Settings" => "\x1b[2m↑/↓ select • Enter toggle • Esc close\x1b[0m",
        ModalMode::Select if modal.title == "Resume Session" => {
            "\x1b[2m↑/↓ select • Enter resume • Ctrl+D delete • Esc cancel\x1b[0m"
        }
        ModalMode::Select if modal.is_searchable => "\x1b[2mEnter to select • Esc to cancel\x1b[0m",
        ModalMode::Select if modal.title.contains("Permission") || modal.title.contains("Approve") => {
            "\x1b[2m↑/↓ select • Enter confirm • Esc deny\x1b[0m"
        }
        ModalMode::Select if modal.allow_custom => {
            "\x1b[2m↑/↓ select • Enter confirm • Esc cancel • or type custom\x1b[0m"
        }
        ModalMode::Select => "\x1b[2m↑/↓ select • Enter confirm • Esc cancel\x1b[0m",
        ModalMode::Input { .. } if modal.options.is_empty() => "\x1b[2mEnter submit • Esc cancel\x1b[0m",
        ModalMode::Input { .. } => "\x1b[2mEnter submit • Esc back\x1b[0m",
    }
}
