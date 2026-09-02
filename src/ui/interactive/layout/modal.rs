use super::text::{visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalState};

pub(crate) fn render_modal_overlay(modal: &ModalState, width: usize) -> (Vec<String>, CursorPosition) {
    let width = width.max(20);
    let inner_width = width.saturating_sub(4).max(1);

    let mut lines = Vec::new();
    let mut cursor = CursorPosition { row: 0, column: 0 };

    if modal.title == "Select Model" {
        lines.push("─".repeat(width));
        let prompt_prefix = "  \x1b[1m>\x1b[0m ";
        let filter_text = &modal.filter_query;
        let prefix_w = visible_width(prompt_prefix);
        let cursor_w = visible_width(filter_text);
        lines.push(format!("{prompt_prefix}{filter_text}"));
        cursor = CursorPosition {
            row: lines.len() - 1,
            column: prefix_w + cursor_w,
        };
        lines.push(String::new());

        if modal.options.is_empty() {
            lines.push("    \x1b[2mNo matching models found\x1b[0m".to_string());
        } else {
            let max_visible = 10;
            let total = modal.options.len();
            let start = if total <= max_visible {
                0
            } else {
                modal.selected.saturating_sub(max_visible / 2).min(total - max_visible)
            };
            let end = (start + max_visible).min(total);

            for i in start..end {
                let opt = &modal.options[i];
                let is_selected = i == modal.selected;

                let (provider, active_mark, default_mark) = if let Some(d) = &opt.description {
                    let mut parts = d.split('\t');
                    (
                        parts.next().unwrap_or(""),
                        parts.next().unwrap_or(""),
                        parts.next().unwrap_or(""),
                    )
                } else {
                    ("", "", "")
                };

                let provider_badge = if !provider.is_empty() {
                    format!(" \x1b[2m[{provider}]\x1b[0m")
                } else {
                    String::new()
                };
                let default_badge = if !default_mark.is_empty() {
                    " \x1b[2m· default\x1b[0m"
                } else {
                    ""
                };
                let check_badge = if !active_mark.is_empty() {
                    " \x1b[32m✓\x1b[0m"
                } else {
                    ""
                };

                if is_selected {
                    let arrow = "\x1b[36m→\x1b[0m ";
                    let model_id_styled = format!("\x1b[36m{}\x1b[0m", opt.label);
                    lines.push(format!(
                        "  {arrow}{model_id_styled}{provider_badge}{default_badge}{check_badge}"
                    ));
                } else {
                    let model_id_styled = &opt.label;
                    lines.push(format!(
                        "    {model_id_styled}{provider_badge}{default_badge}{check_badge}"
                    ));
                }
            }

            if total > max_visible || start > 0 {
                lines.push(format!("    \x1b[2m({}/{})\x1b[0m", modal.selected + 1, total));
            }

            if let Some(selected_opt) = modal.options.get(modal.selected) {
                let desc = selected_opt
                    .description
                    .as_deref()
                    .and_then(|d| d.split('\t').nth(3))
                    .unwrap_or("");
                if !desc.is_empty() {
                    lines.push(String::new());
                    lines.push(format!("  \x1b[2mModel Name: {} ({})\x1b[0m", selected_opt.label, desc));
                }
            }

            lines.push(String::new());
            lines.push("  \x1b[32mModel catalogs refreshed.\x1b[0m".to_string());
        }

        lines.push(String::new());
        lines.push("  \x1b[2mEnter to select • Ctrl+S to set as default • Esc to cancel\x1b[0m".to_string());
        lines.push("─".repeat(width));
        return (lines, cursor);
    }

    lines.push("─".repeat(width));
    lines.push(format!("  \x1b[1;36m{}\x1b[0m", modal.title.trim()));
    lines.push(String::new());

    if !modal.body.trim().is_empty() {
        for line in wrap_to_width(&modal.body, inner_width) {
            lines.push(format!("  {line}"));
        }
        lines.push(String::new());
    }

    if modal.options.is_empty() {
        lines.push("  \x1b[2mNo matching options found\x1b[0m".to_string());
    } else {
        for (i, opt) in modal.options.iter().enumerate() {
            let is_selected = i == modal.selected;
            let prefix = if is_selected { "\x1b[36m▸\x1b[0m " } else { "  " };
            let label_styled = if is_selected {
                format!("\x1b[1m{}\x1b[0m", opt.label)
            } else {
                opt.label.clone()
            };

            let opt_line = if let Some(desc) = &opt.description {
                format!("{prefix}{label_styled}  \x1b[2m{desc}\x1b[0m")
            } else {
                format!("{prefix}{label_styled}")
            };

            if is_selected && modal.mode == ModalMode::Select {
                cursor = CursorPosition {
                    row: lines.len(),
                    column: 2,
                };
            }

            for wrapped in wrap_to_width(&opt_line, inner_width) {
                lines.push(format!("  {wrapped}"));
            }
        }
    }

    if let ModalMode::Input { prompt_label } = &modal.mode {
        lines.push(String::new());
        let prompt_prefix = format!("\x1b[1;36m{prompt_label}:\x1b[0m ");
        let input_text = modal.input.text();
        let prompt_line = format!("{prompt_prefix}{input_text}");

        let prefix_w = visible_width(&prompt_prefix);
        let text_cursor_w = visible_width(&input_text[..modal.input.cursor()]);

        cursor = CursorPosition {
            row: lines.len(),
            column: 2 + prefix_w + text_cursor_w,
        };

        for wrapped in wrap_to_width(&prompt_line, inner_width) {
            lines.push(format!("  {wrapped}"));
        }
    }

    lines.push(String::new());

    let hint = match &modal.mode {
        ModalMode::Select => {
            if modal.title.contains("Permission") || modal.title.contains("Approve") {
                "\x1b[2m↑/↓ select • Enter confirm • Esc deny\x1b[0m"
            } else if modal.allow_custom {
                "\x1b[2m↑/↓ select • Enter confirm • Esc cancel • or type custom\x1b[0m"
            } else {
                "\x1b[2m↑/↓ select • Enter confirm • Esc cancel\x1b[0m"
            }
        }
        ModalMode::Input { .. } => {
            if modal.options.is_empty() {
                "\x1b[2mEnter submit • Esc cancel\x1b[0m"
            } else {
                "\x1b[2mEnter submit • Esc back\x1b[0m"
            }
        }
    };
    lines.push(format!("  {hint}"));
    lines.push("─".repeat(width));

    (lines, cursor)
}
