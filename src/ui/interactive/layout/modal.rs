use crate::ui::interactive::layout::editor::{render_editor_lines, window_editor, wrap_editor};
use crate::ui::interactive::layout::text::{truncate_to_width, visible_width, wrap_to_width};
use crate::ui::interactive::{CursorPosition, ModalMode, ModalOption, ModalState, OptionLayout};

pub struct OptionFormat<'a> {
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

pub fn modal_hint(modal: &ModalState) -> &'static str {
    match &modal.mode {
        crate::ui::interactive::ModalMode::Select => select_modal_hint(modal),
        crate::ui::interactive::ModalMode::Input { .. } if modal.options.is_empty() => {
            "Enter submit • Shift+Enter newline • Esc cancel"
        }
        crate::ui::interactive::ModalMode::Input { .. } => "Enter submit • Shift+Enter newline • Esc back",
    }
}

pub struct ModalOptionsLayout<'a> {
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
    let (start, end, is_paginated) = calculate_pagination(total, modal.selected, layout.max_visible);
    let mut lines = render_visible_options(modal, &layout, (start, end));
    if is_paginated {
        let dim = layout.theme.dimmed;
        lines.push(format!("    {dim}(showing {}-{} of {total}){dim:#}", start + 1, end));
    }
    lines
}

// --- Horizontal Options Layout ---

fn horizontal_item_width(opt: &ModalOption, is_selected: bool) -> usize {
    visible_width(&opt.label) + if is_selected { 2 } else { 0 }
}

fn window_width(modal: &ModalState, start: usize, end: usize) -> usize {
    let mut width = 0;
    if start > 0 {
        width += 2;
    }
    for i in start..=end {
        width += horizontal_item_width(&modal.options[i], i == modal.selected);
        if i < end {
            width += 3;
        }
    }
    if end + 1 < modal.options.len() {
        width += 2;
    }
    width
}

fn expand_window(modal: &ModalState, inner_width: usize, (mut start, mut end): (usize, usize)) -> (usize, usize) {
    let (n, sel) = (
        modal.options.len(),
        modal.selected.min(modal.options.len().saturating_sub(1)),
    );
    loop {
        let can_left = start > 0 && window_width(modal, start - 1, end) <= inner_width;
        let can_right = end + 1 < n && window_width(modal, start, end + 1) <= inner_width;
        if !can_left && !can_right {
            break;
        }
        if can_left && can_right {
            if sel - start <= end - sel {
                start -= 1;
            } else {
                end += 1;
            }
        } else if can_left {
            start -= 1;
        } else {
            end += 1;
        }
    }
    (start, end)
}

pub fn calculate_horizontal_window(modal: &ModalState, inner_width: usize) -> (usize, usize) {
    let n = modal.options.len();
    if n == 0 {
        return (0, 0);
    }
    let sel = modal.selected.min(n - 1);
    if window_width(modal, 0, n - 1) <= inner_width {
        return (0, n - 1);
    }
    expand_window(modal, inner_width, (sel, sel))
}

fn format_horizontal_option(opt: &ModalOption, is_selected: bool, theme: &crate::ui::theme::Theme) -> String {
    if is_selected {
        let hl = theme.highlight;
        format!("{hl}▸{hl:#} \x1b[1m{}\x1b[0m", opt.label)
    } else {
        opt.label.clone()
    }
}

pub fn format_horizontal_row(
    modal: &ModalState,
    layout: &ModalOptionsLayout<'_>,
    (start, end): (usize, usize),
) -> String {
    let mut parts = Vec::new();
    if start > 0 {
        parts.push(format!("{}‹{} ", layout.theme.dimmed, layout.theme.dimmed));
    }
    for i in start..=end {
        parts.push(format_horizontal_option(
            &modal.options[i],
            i == modal.selected,
            layout.theme,
        ));
        if i < end {
            parts.push("   ".to_string());
        }
    }
    if end + 1 < modal.options.len() {
        parts.push(format!(" {}›{}", layout.theme.dimmed, layout.theme.dimmed));
    }
    parts.concat()
}

pub fn render_horizontal_options(modal: &ModalState, layout: &ModalOptionsLayout<'_>) -> Vec<String> {
    if layout.max_visible == 0 {
        return Vec::new();
    }
    if modal.options.is_empty() {
        let msg = if modal.is_searchable {
            "No matching models found"
        } else {
            "No matching options found"
        };
        let dimmed = layout.theme.dimmed;
        return vec![format!("    {dimmed}{msg}{dimmed:#}")];
    }
    let window = calculate_horizontal_window(modal, layout.inner_width);
    let row = format_horizontal_row(modal, layout, window);
    vec![format!("  {row}")]
}

// --- In-Input Modal Rendering ---

fn input_prompt_width(modal: &ModalState) -> usize {
    let ModalMode::Input { prompt_label } = &modal.mode else {
        return 0;
    };
    2 + visible_width(prompt_label) + 3
}

fn input_wrap_width(modal: &ModalState, inner_width: usize) -> usize {
    inner_width
        .saturating_add(4)
        .saturating_sub(input_prompt_width(modal))
        .max(1)
}

fn options_desired_lines(modal: &ModalState) -> usize {
    if matches!(modal.mode, ModalMode::Input { .. }) {
        0
    } else if modal.options.is_empty() {
        usize::from(modal.is_searchable)
    } else if modal.option_layout == OptionLayout::Horizontal {
        1
    } else {
        modal.options.len().min(12)
    }
}

pub fn modal_body_max_scroll(modal: &ModalState, draft_text: &str, width: usize, height: usize) -> usize {
    let inner_width = width.saturating_sub(4).max(1);
    let total = wrap_to_width(&modal.body, inner_width).len();
    if total == 0 {
        return 0;
    }
    let desired = in_input_modal_desired_lines(modal, draft_text, inner_width);
    let budget = crate::ui::interactive::layout::budget::compute_normal_budget(
        &crate::ui::interactive::layout::budget::NormalBudgetInput {
            terminal_height: height,
            raw_queued_count: 0,
            raw_widgets_count: 0,
            raw_footer_count: 1,
            total_editor_lines: desired,
            autocomplete_desired: 0,
            is_modal: true,
        },
    );
    let (b_space, _, _, _) = modal_in_input_spaces(modal, draft_text, (budget.editor_max_lines, inner_width));
    total.saturating_sub(b_space.saturating_sub(1))
}

pub fn in_input_modal_desired_lines(modal: &ModalState, draft_text: &str, inner_width: usize) -> usize {
    let search = usize::from(modal.is_searchable);
    let input = if matches!(modal.mode, ModalMode::Input { .. }) {
        wrap_to_width(modal.input.text(), input_wrap_width(modal, inner_width))
            .len()
            .max(1)
    } else {
        0
    };
    let draft = usize::from(!draft_text.trim().is_empty());
    let body = if modal.body.trim().is_empty() {
        0
    } else {
        wrap_to_width(&modal.body, inner_width).len()
    };
    let options = options_desired_lines(modal);
    (search + input + draft + body + options).max(1)
}

pub struct InInputModalInput<'a> {
    pub modal: &'a ModalState,
    pub draft_text: &'a str,
    pub bounds: (usize, usize),
    pub theme: &'a crate::ui::theme::Theme,
    pub focused: bool,
}

pub fn calculate_content_space(modal: &ModalState, space_for_content: usize) -> (usize, usize) {
    if matches!(modal.mode, ModalMode::Input { .. }) || modal.options.is_empty() {
        (space_for_content, 0)
    } else if modal.option_layout == OptionLayout::Horizontal {
        let opt_space = 1.min(space_for_content);
        (space_for_content.saturating_sub(opt_space), opt_space)
    } else if modal.body.trim().is_empty() || space_for_content <= 2 {
        (0, space_for_content)
    } else {
        let opt_desired = modal
            .options
            .len()
            .min(5)
            .min(space_for_content.saturating_sub(1))
            .max(1);
        (space_for_content.saturating_sub(opt_desired), opt_desired)
    }
}

fn push_search_row(
    modal: &ModalState,
    width: usize,
    (focused, cursor_mode): (bool, crate::ui::theme::CursorMode),
    lines: &mut Vec<String>,
) -> (CursorPosition, bool) {
    let query = truncate_to_width(&modal.filter_query, width.saturating_sub(6));
    let cursor = CursorPosition {
        row: lines.len(),
        column: (visible_width("  > ") + visible_width(&query)).min(width),
    };
    if focused && cursor_mode == crate::ui::theme::CursorMode::Software {
        lines.push(format!("  \x1b[1m>\x1b[0m {query}\x1b[7m \x1b[27m"));
    } else {
        lines.push(format!("  \x1b[1m>\x1b[0m {query}"));
    }
    (cursor, focused)
}

fn input_prompt_prefix(modal: &ModalState, theme: &crate::ui::theme::Theme) -> (String, usize) {
    let ModalMode::Input { prompt_label } = &modal.mode else {
        return (String::new(), 0);
    };
    let dim = theme.dimmed;
    let accent = theme.highlight;
    let styled = format!("  {dim}{prompt_label}{dim:#} {accent}›{accent:#} ");
    (styled, input_prompt_width(modal))
}

fn push_modal_input_prompt(
    modal: &ModalState,
    theme: &crate::ui::theme::Theme,
    (width, max_input_lines, focused, lines): (usize, usize, bool, &mut Vec<String>),
) -> Option<(CursorPosition, bool)> {
    let ModalMode::Input { .. } = &modal.mode else {
        return None;
    };
    let (styled_prefix, prefix_width) = input_prompt_prefix(modal, theme);
    let edit_width = width.saturating_sub(prefix_width).max(1);

    let (wrapped, cursor_pos) = wrap_editor(&modal.input, edit_width);
    let (windowed, cur) = window_editor(wrapped, cursor_pos, max_input_lines.max(1));
    let windowed = if focused && theme.cursor_mode == crate::ui::theme::CursorMode::Software {
        render_editor_lines(windowed, cur)
    } else {
        windowed
    };
    let base_row = lines.len();

    let continuation = " ".repeat(prefix_width);
    for (offset, line) in windowed.into_iter().enumerate() {
        if offset == 0 {
            lines.push(format!("{styled_prefix}{line}"));
        } else {
            lines.push(format!("{continuation}{line}"));
        }
    }
    let cursor = CursorPosition {
        row: base_row + cur.row,
        column: (prefix_width + cur.column).min(width),
    };
    Some((cursor, focused))
}

fn format_draft_line(draft_text: &str, inner_width: usize, theme: &crate::ui::theme::Theme) -> String {
    let single_line = draft_text.trim().replace('\n', " ");
    let max_preview = inner_width.saturating_sub(29).max(5);
    let preview = truncate_to_width(&single_line, max_preview);
    let dimmed = theme.dimmed;
    let line = format!("  {dimmed}Draft: \"{preview}\" (restores on close){dimmed:#}");
    wrap_to_width(&line, inner_width.saturating_add(2)).remove(0)
}

fn collect_modal_content(
    input: &InInputModalInput<'_>,
    (inner_width, body_space, opt_space): (usize, usize, usize),
    has_draft: bool,
) -> Vec<String> {
    let mut lines = render_in_input_body(input.modal, inner_width, body_space);
    if opt_space > 0 {
        lines.extend(render_modal_options(
            input.modal,
            ModalOptionsLayout {
                inner_width,
                max_visible: opt_space,
                theme: input.theme,
            },
        ));
    }
    if has_draft {
        lines.push(format_draft_line(input.draft_text, inner_width, input.theme));
    }
    lines
}

fn modal_in_input_spaces(
    modal: &ModalState,
    draft_text: &str,
    (max_lines, inner_width): (usize, usize),
) -> (usize, usize, usize, bool) {
    let has_draft = !draft_text.trim().is_empty() && max_lines >= 2;
    let has_search = modal.is_searchable && !matches!(modal.mode, ModalMode::Input { .. });
    let is_input = matches!(modal.mode, ModalMode::Input { .. });
    let input_lines = if is_input {
        wrap_to_width(modal.input.text(), input_wrap_width(modal, inner_width))
            .len()
            .max(1)
            .min(max_lines.saturating_sub(2).max(1))
    } else {
        0
    };
    let fixed = usize::from(has_search) + input_lines + usize::from(has_draft);
    let (b_space, opt_space) = calculate_content_space(modal, max_lines.saturating_sub(fixed));
    (b_space, opt_space, input_lines, has_draft)
}

pub fn render_in_input_modal(input: InInputModalInput<'_>) -> (Vec<String>, CursorPosition, bool) {
    let (width, max_lines) = (input.bounds.0.max(1), input.bounds.1);
    if max_lines == 0 {
        return (Vec::new(), CursorPosition { row: 0, column: 0 }, false);
    }
    let inner_width = width.saturating_sub(4).max(1);
    let (b_space, opt_space, input_lines, has_draft) =
        modal_in_input_spaces(input.modal, input.draft_text, (max_lines, inner_width));

    let mut lines = Vec::new();
    let mut cursor = (CursorPosition { row: 0, column: 0 }, false);
    if input.modal.is_searchable && !matches!(input.modal.mode, ModalMode::Input { .. }) {
        cursor = push_search_row(input.modal, width, (input.focused, input.theme.cursor_mode), &mut lines);
    }
    lines.extend(collect_modal_content(
        &input,
        (inner_width, b_space, opt_space),
        has_draft,
    ));
    if let Some(c) = push_modal_input_prompt(
        input.modal,
        input.theme,
        (width, input_lines, input.focused, &mut lines),
    ) {
        cursor = c;
    }
    (lines, cursor.0, cursor.1)
}

fn format_scroll_indicator(current_line: usize, total_lines: usize) -> String {
    format!("  \x1b[2m↑/↓ scroll (line {current_line}/{total_lines})\x1b[0m")
}

fn render_horizontal_body_lines(wrapped: Vec<String>, body_scroll: usize, space: usize) -> Vec<String> {
    let total = wrapped.len();
    if space == 1 {
        return vec![format_scroll_indicator(1, total)];
    }
    let visible_count = space - 1;
    let max_scroll = total.saturating_sub(visible_count);
    let scroll = body_scroll.min(max_scroll);
    let mut lines: Vec<String> = wrapped
        .into_iter()
        .skip(scroll)
        .take(visible_count)
        .map(|l| format!("  {l}"))
        .collect();
    lines.push(format_scroll_indicator(scroll + 1, total));
    lines
}

fn render_vertical_omission_lines(wrapped: Vec<String>, space: usize) -> Vec<String> {
    let total = wrapped.len();
    if space == 1 {
        vec![format_omission_line(total)]
    } else {
        let visible = space - 1;
        let omitted = total - visible;
        let mut lines: Vec<String> = wrapped.into_iter().take(visible).map(|l| format!("  {l}")).collect();
        lines.push(format_omission_line(omitted));
        lines
    }
}

fn render_in_input_body(modal: &ModalState, inner_width: usize, space: usize) -> Vec<String> {
    if modal.body.trim().is_empty() || space == 0 {
        return Vec::new();
    }
    let wrapped = wrap_to_width(&modal.body, inner_width);
    let total = wrapped.len();
    if total == 0 {
        return Vec::new();
    }
    if total <= space {
        wrapped.into_iter().map(|line| format!("  {line}")).collect()
    } else if modal.option_layout == OptionLayout::Horizontal {
        render_horizontal_body_lines(wrapped, modal.body_scroll, space)
    } else {
        render_vertical_omission_lines(wrapped, space)
    }
}

fn format_omission_line(omitted: usize) -> String {
    format!("  \x1b[2m[... {omitted} lines omitted ...]\x1b[0m")
}

#[cfg(test)]
pub mod in_input {
    pub use super::*;
}

#[cfg(test)]
pub mod options {
    pub use super::*;
}
