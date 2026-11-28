use std::collections::HashSet;

use anstyle::Style;
use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, A2uiSurface, BoundString, ButtonVariant, MAX_DEPTH, RowJustify, SemanticTone,
    TextUsageHint, resolve_pointer, resolve_typed,
};

use super::modal::A2uiModalState;
use super::modal::view::{
    ChoicePickerRender, TextFieldRender, render_modal_button, render_modal_checkbox, render_modal_choice_picker,
    render_modal_text_field,
};
use crate::ui::block::visible_width;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::footer::truncate_to_width;
use crate::ui::interactive::layout::wrap_to_width;
use crate::ui::render::diff::replace_tabs;
use crate::ui::theme::SGR_RESET;
use crate::ui::theme::Theme;

const STATIC_MASK_CAP: usize = 12;

#[derive(Clone, Copy)]
pub(super) struct ModalFocus<'a> {
    pub(super) state: &'a A2uiModalState,
    pub(super) focused: bool,
}

pub(super) struct RenderCtx<'a, 'b> {
    pub(super) surface: &'a A2uiSurface,
    depth: usize,
    active: &'b mut HashSet<&'a str>,
    pub(super) width: usize,
    pub(super) theme: &'a Theme,
    pub(super) modal: Option<ModalFocus<'a>>,
    pub(super) cursor: Option<CursorPosition>,
    pub(super) focus_range: Option<(usize, usize)>,
}

impl<'a> RenderCtx<'a, '_> {
    fn child(&mut self, width: usize) -> RenderCtx<'a, '_> {
        RenderCtx {
            surface: self.surface,
            depth: self.depth + 1,
            active: self.active,
            width,
            theme: self.theme,
            modal: self.modal,
            cursor: None,
            focus_range: None,
        }
    }
}

pub(super) struct RenderedTree {
    pub(super) lines: Vec<String>,
    pub(super) cursor: Option<CursorPosition>,
    pub(super) focus_range: Option<(usize, usize)>,
}

pub(super) fn walk_tree<'a>(
    surface: &'a A2uiSurface,
    width: usize,
    theme: &'a Theme,
    modal: Option<ModalFocus<'a>>,
) -> RenderedTree {
    let mut active = HashSet::new();
    let mut ctx = RenderCtx {
        surface,
        depth: 0,
        active: &mut active,
        width,
        theme,
        modal,
        cursor: None,
        focus_range: None,
    };
    let lines = render_node(&mut ctx, &surface.root_id);
    RenderedTree {
        lines,
        cursor: ctx.cursor,
        focus_range: ctx.focus_range,
    }
}

pub fn render_surface(surface: &A2uiSurface, width: usize, theme: &Theme) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    walk_tree(surface, width, theme, None)
        .lines
        .into_iter()
        .map(|line| clamp_line(&line, width))
        .collect()
}

pub fn render_surfaces(surfaces: &[&A2uiSurface], width: usize, max_lines: usize, theme: &Theme) -> Vec<String> {
    if surfaces.is_empty() || width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut all_lines = Vec::new();
    for surface in surfaces {
        all_lines.extend(render_surface(surface, width, theme));
    }
    truncate_lines(all_lines, max_lines, width, theme)
}

fn truncate_lines(mut lines: Vec<String>, max_lines: usize, width: usize, theme: &Theme) -> Vec<String> {
    if lines.len() <= max_lines {
        return lines;
    }
    let keep = max_lines.saturating_sub(1);
    let hidden = lines.len() - keep;
    lines.truncate(keep);
    let notice = more_lines_notice(hidden, theme);
    lines.push(clamp_line(&notice, width));
    lines
}

pub(super) fn more_lines_notice(hidden: usize, theme: &Theme) -> String {
    let dim = theme.dimmed;
    format!("{dim}... (+{hidden} lines){dim:#}")
}

pub(super) fn clamp_line(line: &str, width: usize) -> String {
    if visible_width(line) <= width {
        return line.to_string();
    }
    let truncated = truncate_to_width(line, width);
    if truncated.contains('\x1b') {
        format!("{truncated}{SGR_RESET}")
    } else {
        truncated
    }
}

pub(super) fn style_line(text: &str, style: Style) -> String {
    if style == Style::new() {
        text.to_string()
    } else {
        format!("{style}{text}{style:#}")
    }
}

pub(super) fn tone_style(tone: SemanticTone, theme: &Theme) -> Style {
    match tone {
        SemanticTone::Neutral => Style::new(),
        SemanticTone::Accent => theme.highlight,
        SemanticTone::Success => theme.tool_ok,
        SemanticTone::Warning => theme.warning,
        SemanticTone::Error => theme.tool_err,
    }
}

pub(super) fn card_border_style(tone: Option<SemanticTone>, theme: &Theme) -> Style {
    match tone {
        None | Some(SemanticTone::Neutral) => theme.agent_border,
        Some(t) => tone_style(t, theme),
    }
}

/// `active` holds the ids on the current path, so a cycle is cut while a shared child still renders per parent.
fn render_node<'a>(ctx: &mut RenderCtx<'a, '_>, id: &'a str) -> Vec<String> {
    if ctx.depth >= MAX_DEPTH || ctx.width == 0 || !ctx.active.insert(id) {
        return Vec::new();
    }
    let lines = match ctx.surface.component(id) {
        Some(component) => render_component(ctx, component),
        None => Vec::new(),
    };
    ctx.active.remove(id);
    lines
}

fn render_component<'a>(ctx: &mut RenderCtx<'a, '_>, component: &'a A2uiComponent) -> Vec<String> {
    match &component.kind {
        A2uiComponentKind::Column { children, gap } => render_column(ctx, children, gap.unwrap_or(0) as usize),
        A2uiComponentKind::Row { children, gap, justify } => {
            render_row(ctx, children, gap.unwrap_or(1) as usize, justify.unwrap_or_default())
        }
        A2uiComponentKind::Card { title, child, tone } => render_card(ctx, title.as_ref(), child, *tone),
        A2uiComponentKind::Divider { label } => render_divider(ctx.surface, label.as_ref(), ctx.width, ctx.theme),
        A2uiComponentKind::Text { text, usage_hint, tone } => {
            render_text(ctx.surface, text, *usage_hint, *tone, ctx.width, ctx.theme)
        }
        A2uiComponentKind::Code { code, language } => {
            render_code(ctx.surface, code, language.as_deref(), ctx.width, ctx.theme)
        }
        A2uiComponentKind::Badge { label, tone } => render_badge(ctx.surface, label, *tone, ctx.width, ctx.theme),
        A2uiComponentKind::Table { headers, rows } => render_table(ctx.surface, headers, rows, ctx.width, ctx.theme),
        A2uiComponentKind::Button { label, variant, .. } => match ctx.modal {
            Some(modal) => render_modal_button(ctx, modal, &component.id, label, *variant),
            None => render_button(ctx.surface, label, *variant, ctx.width, ctx.theme),
        },
        A2uiComponentKind::ChoicePicker {
            label,
            options,
            selection_path,
            multi_select,
            allow_other,
            other_path,
        } => match ctx.modal {
            Some(modal) => render_modal_choice_picker(
                ctx,
                modal,
                ChoicePickerRender {
                    id: &component.id,
                    label: label.as_ref(),
                    options,
                    selection_path,
                    multi_select: *multi_select,
                    allow_other: *allow_other,
                    other_path: other_path.as_deref(),
                },
            ),
            None => render_choice_picker(ctx.surface, label.as_ref(), options, ctx.width, ctx.theme),
        },
        A2uiComponentKind::TextField {
            label,
            placeholder,
            value_path,
            masked,
            ..
        } => match ctx.modal {
            Some(modal) => render_modal_text_field(
                ctx,
                modal,
                TextFieldRender {
                    id: &component.id,
                    label: label.as_ref(),
                    placeholder: placeholder.as_deref(),
                    value_path,
                    masked: *masked,
                },
            ),
            None => render_text_field(ctx, label.as_ref(), placeholder.as_deref(), value_path, *masked),
        },
        A2uiComponentKind::CheckBox { label, checked_path } => match ctx.modal {
            Some(modal) => render_modal_checkbox(ctx, modal, &component.id, label, checked_path),
            None => render_checkbox(ctx.surface, label, checked_path, ctx.width, ctx.theme),
        },
    }
}

fn merge_child_offsets(
    ctx: &mut RenderCtx<'_, '_>,
    child_cursor: Option<CursorPosition>,
    child_focus: Option<(usize, usize)>,
    base_row: usize,
    col_offset: usize,
) {
    if let Some(c) = child_cursor {
        ctx.cursor = Some(CursorPosition {
            row: base_row + c.row,
            column: col_offset + c.column,
        });
    }
    if let Some((start, end)) = child_focus {
        ctx.focus_range = Some((base_row + start, base_row + end));
    }
}

fn render_column<'a>(ctx: &mut RenderCtx<'a, '_>, children: &'a [String], gap: usize) -> Vec<String> {
    let mut out = Vec::new();
    let width = ctx.width;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let rendered = render_node(&mut child_ctx, child_id);
        let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
        if rendered.is_empty() {
            continue;
        }
        if !out.is_empty() && gap > 0 {
            out.extend(std::iter::repeat_n(String::new(), gap));
        }
        let base_row = out.len();
        merge_child_offsets(ctx, child_cursor, child_focus, base_row, 0);
        out.extend(rendered);
    }
    out
}

fn render_row<'a>(ctx: &mut RenderCtx<'a, '_>, children: &'a [String], gap: usize, justify: RowJustify) -> Vec<String> {
    let mut segments = Vec::new();
    let mut cursor = None;
    let width = ctx.width;
    let mut has_focus = false;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let lines = render_node(&mut child_ctx, child_id);
        if lines.is_empty() {
            continue;
        }
        has_focus |= child_ctx.focus_range.is_some();
        if let Some(c) = child_ctx.cursor {
            cursor = Some((segments.len(), joined_column(&lines, c)));
        }
        segments.push(lines.join(" "));
    }
    if segments.is_empty() {
        return Vec::new();
    }
    if has_focus {
        ctx.focus_range = Some((0, 1));
    }
    let (line, starts) = layout_row_segments(&segments, gap, justify, width);
    if let Some((idx, column)) = cursor {
        ctx.cursor = Some(CursorPosition {
            row: 0,
            column: starts[idx] + column,
        });
    }
    vec![line]
}

fn joined_column(lines: &[String], cursor: CursorPosition) -> usize {
    let before: usize = lines.iter().take(cursor.row).map(|l| visible_width(l) + 1).sum();
    before + cursor.column
}

pub(super) fn layout_row_segments(
    segments: &[String],
    gap: usize,
    justify: RowJustify,
    width: usize,
) -> (String, Vec<usize>) {
    let widths: Vec<usize> = segments.iter().map(|s| visible_width(s)).collect();
    let (lead, between) = row_spacing(&widths, gap, justify, width);
    let mut out = " ".repeat(lead);
    let mut starts = Vec::with_capacity(segments.len());
    let mut col = lead;
    for (idx, segment) in segments.iter().enumerate() {
        if idx > 0 {
            let spaces = between[idx - 1];
            out.push_str(&" ".repeat(spaces));
            col += spaces;
        }
        starts.push(col);
        out.push_str(segment);
        col += widths[idx];
    }
    (clamp_line(&out, width), starts)
}

fn row_spacing(widths: &[usize], gap: usize, justify: RowJustify, width: usize) -> (usize, Vec<usize>) {
    let total: usize = widths.iter().sum();
    let gaps = widths.len().saturating_sub(1);
    if justify == RowJustify::SpaceBetween && gaps > 0 && total + gaps <= width {
        let free = width - total;
        return (
            0,
            (0..gaps).map(|i| free / gaps + usize::from(i < free % gaps)).collect(),
        );
    }
    let free = width.saturating_sub(total + gap * gaps);
    let lead = match justify {
        RowJustify::Center => free / 2,
        RowJustify::End => free,
        RowJustify::Start | RowJustify::SpaceBetween => 0,
    };
    (lead, vec![gap; gaps])
}

fn render_card<'a>(
    ctx: &mut RenderCtx<'a, '_>,
    title: Option<&BoundString>,
    child: &'a str,
    tone: Option<SemanticTone>,
) -> Vec<String> {
    let width = ctx.width;
    if width < 4 {
        let mut child_ctx = ctx.child(width);
        let out = render_node(&mut child_ctx, child);
        let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
        merge_child_offsets(ctx, child_cursor, child_focus, 0, 0);
        return out;
    }
    let mut child_ctx = ctx.child(card_inner_width(width));
    let child_lines = render_node(&mut child_ctx, child);
    let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
    merge_child_offsets(ctx, child_cursor, child_focus, 1, 2);
    let border = card_border_style(tone, ctx.theme);
    card_frame(ctx.surface, title, width, border, child_lines)
}

pub(super) fn card_inner_width(width: usize) -> usize {
    width.saturating_sub(4).max(1)
}

pub(super) fn card_frame(
    surface: &A2uiSurface,
    title: Option<&BoundString>,
    width: usize,
    border: Style,
    child_lines: Vec<String>,
) -> Vec<String> {
    let inner_width = card_inner_width(width);
    let mut out = Vec::with_capacity(child_lines.len() + 2);
    out.push(card_top_border(surface, title, width, border));
    for line in child_lines {
        let clamped = clamp_line(&line, inner_width);
        let pad = inner_width.saturating_sub(visible_width(&clamped));
        out.push(format!(
            "{border}│{border:#} {clamped}{} {border}│{border:#}",
            " ".repeat(pad)
        ));
    }
    let bottom = format!("╰{}╯", "─".repeat(width.saturating_sub(2)));
    out.push(style_line(&bottom, border));
    out
}

pub(super) fn card_top_border(
    surface: &A2uiSurface,
    title: Option<&BoundString>,
    width: usize,
    border: Style,
) -> String {
    let inner = width.saturating_sub(2);
    let title_text = title
        .map(|t| surface.resolve_string(t))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let Some(title_text) = title_text else {
        return style_line(&format!("╭{}╮", "─".repeat(inner)), border);
    };
    let max_title = inner.saturating_sub(3);
    if max_title == 0 {
        return style_line(&format!("╭{}╮", "─".repeat(inner)), border);
    }
    let clamped_title = truncate_to_width(&title_text, max_title);
    let used = 3 + visible_width(&clamped_title);
    let trail = inner.saturating_sub(used);
    style_line(&format!("╭─ {clamped_title} {}╮", "─".repeat(trail)), border)
}

pub(super) fn render_divider(
    surface: &A2uiSurface,
    label: Option<&BoundString>,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let text = label
        .map(|l| surface.resolve_string(l))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty());
    let line = match text {
        Some(lbl) if width > 4 => {
            let clamped = truncate_to_width(&lbl, width.saturating_sub(4));
            let trail = width.saturating_sub(visible_width(&clamped) + 4);
            format!("── {clamped} {}", "─".repeat(trail))
        }
        _ => "─".repeat(width),
    };
    vec![style_line(&line, theme.dimmed)]
}

pub(super) fn render_text(
    surface: &A2uiSurface,
    text: &BoundString,
    usage_hint: TextUsageHint,
    tone: Option<SemanticTone>,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let resolved = surface.resolve_string(text);
    if resolved.is_empty() {
        return Vec::new();
    }
    let style = text_style(usage_hint, tone, theme);
    wrap_to_width(&resolved, width)
        .into_iter()
        .map(|line| style_line(&line, style))
        .collect()
}

fn text_style(usage_hint: TextUsageHint, tone: Option<SemanticTone>, theme: &Theme) -> Style {
    let base = match usage_hint {
        TextUsageHint::H1 => theme.heading_h1.bold(),
        TextUsageHint::H2 => theme.heading_h2.bold(),
        TextUsageHint::H3 => theme.heading_h3.bold(),
        TextUsageHint::Body => Style::new(),
        TextUsageHint::Caption | TextUsageHint::Muted => theme.dimmed,
    };
    match tone {
        Some(t) if t != SemanticTone::Neutral => {
            let t_style = tone_style(t, theme);
            if matches!(usage_hint, TextUsageHint::H1 | TextUsageHint::H2 | TextUsageHint::H3) {
                t_style.bold()
            } else {
                t_style
            }
        }
        _ => base,
    }
}

pub(super) fn render_code(
    surface: &A2uiSurface,
    code: &BoundString,
    language: Option<&str>,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let resolved = replace_tabs(&surface.resolve_string(code));
    if resolved.is_empty() {
        return Vec::new();
    }
    resolved
        .lines()
        .map(|line| {
            let clamped = truncate_to_width(line, width);
            if theme.syntax_highlight_tools {
                crate::ui::markdown::highlight_code_line(&clamped, language, theme)
            } else {
                style_line(&clamped, theme.code_inline)
            }
        })
        .collect()
}

pub(super) fn render_badge(
    surface: &A2uiSurface,
    label: &BoundString,
    tone: SemanticTone,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let resolved = surface.resolve_string(label);
    let badge = format!("[{}]", resolved.trim());
    let styled = style_line(&badge, tone_style(tone, theme));
    vec![clamp_line(&styled, width)]
}

pub(super) fn render_table(
    surface: &A2uiSurface,
    headers: &[BoundString],
    rows: &rho_harness_core::a2ui::BoundTableRows,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let resolved_headers: Vec<String> = headers.iter().map(|h| surface.resolve_string(h)).collect();
    let resolved_rows: Vec<Vec<String>> = resolve_typed(&surface.data_model, rows).unwrap_or_default();
    let col_count = resolved_headers
        .len()
        .max(resolved_rows.iter().map(Vec::len).max().unwrap_or(0));
    if col_count == 0 {
        return Vec::new();
    }
    let widths = compute_table_widths(&resolved_headers, &resolved_rows, col_count, width);
    let mut out = Vec::new();
    if !resolved_headers.is_empty() {
        let header_line = format_table_row(&resolved_headers, &widths);
        out.push(style_line(&header_line, Style::new().bold()));
        let sep: Vec<String> = widths.iter().map(|&w| "─".repeat(w)).collect();
        out.push(style_line(&sep.join("─┼─"), theme.dimmed));
    }
    for row in &resolved_rows {
        out.push(format_table_row(row, &widths));
    }
    out
}

fn compute_table_widths(headers: &[String], rows: &[Vec<String>], col_count: usize, max_width: usize) -> Vec<usize> {
    let mut widths = vec![1usize; col_count];
    for (idx, h) in headers.iter().enumerate() {
        widths[idx] = widths[idx].max(visible_width(h));
    }
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            if idx < col_count {
                widths[idx] = widths[idx].max(visible_width(cell));
            }
        }
    }
    let sep_total = col_count.saturating_sub(1) * 3;
    let avail = max_width.saturating_sub(sep_total).max(col_count);
    let total: usize = widths.iter().sum();
    if total > avail {
        for w in &mut widths {
            *w = ((*w * avail) / total.max(1)).max(1);
        }
    }
    widths
}

fn format_table_row(cells: &[String], widths: &[usize]) -> String {
    let mut formatted = Vec::with_capacity(widths.len());
    for (idx, &col_w) in widths.iter().enumerate() {
        let cell = cells.get(idx).map(String::as_str).unwrap_or("");
        let clamped = truncate_to_width(cell, col_w);
        let pad = col_w.saturating_sub(visible_width(&clamped));
        formatted.push(format!("{clamped}{}", " ".repeat(pad)));
    }
    formatted.join(" │ ")
}

fn render_button(
    surface: &A2uiSurface,
    label: &BoundString,
    variant: ButtonVariant,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let resolved = surface.resolve_string(label);
    let style = match variant {
        ButtonVariant::Primary => theme.highlight.bold(),
        ButtonVariant::Secondary => theme.dimmed,
        ButtonVariant::Danger => theme.tool_err.bold(),
    };
    vec![clamp_line(&style_line(&format!("[ {resolved} ]"), style), width)]
}

pub(super) fn render_optional_label(
    surface: &A2uiSurface,
    label: Option<&BoundString>,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    label
        .map(|lbl| render_text(surface, lbl, TextUsageHint::Body, None, width, theme))
        .unwrap_or_default()
}

fn render_choice_picker(
    surface: &A2uiSurface,
    label: Option<&BoundString>,
    options: &[rho_harness_core::a2ui::ChoiceOption],
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let mut out = render_optional_label(surface, label, width, theme);
    for opt in options {
        let opt_label = surface.resolve_string(&opt.label);
        out.push(clamp_line(&format!("  ○ {opt_label}"), width));
    }
    out
}

pub(super) fn mask_text(value: &str, max_stars: Option<usize>) -> String {
    let count = value.chars().count();
    "*".repeat(max_stars.map_or(count, |cap| count.min(cap)))
}

fn render_text_field(
    ctx: &RenderCtx<'_, '_>,
    label: Option<&BoundString>,
    placeholder: Option<&str>,
    value_path: &str,
    masked: bool,
) -> Vec<String> {
    let val = resolve_pointer(&ctx.surface.data_model, value_path)
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let display = if val.is_empty() {
        placeholder.unwrap_or("").to_string()
    } else if masked {
        mask_text(val, Some(STATIC_MASK_CAP))
    } else {
        val.to_string()
    };
    let prefix = label
        .map(|l| format!("{}: ", ctx.surface.resolve_string(l)))
        .unwrap_or_default();
    let line = format!("{prefix}[{display}]");
    vec![clamp_line(&style_line(&line, ctx.theme.dimmed), ctx.width)]
}

fn render_checkbox(
    surface: &A2uiSurface,
    label: &BoundString,
    checked_path: &str,
    width: usize,
    theme: &Theme,
) -> Vec<String> {
    let checked = resolve_pointer(&surface.data_model, checked_path)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mark = if checked { "[x]" } else { "[ ]" };
    let text = surface.resolve_string(label);
    let line = format!("{mark} {text}");
    let style = if checked { theme.tool_ok } else { Style::new() };
    vec![clamp_line(&style_line(&line, style), width)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::strip_ansi;
    use crate::ui::interactive::a2ui::test_support::create_surface;
    use rho_harness_core::a2ui::{
        A2uiMessage, BoundValue, SurfaceStore, UpdateComponents, UpdateDataModel, todo_surface_messages,
    };
    use rho_harness_core::todo::{TodoItem, TodoStatus};
    use serde_json::json;

    fn build_surface(components: Vec<A2uiComponent>, data: serde_json::Value) -> A2uiSurface {
        let mut store = SurfaceStore::new();
        store
            .apply_all([
                create_surface("test", "root", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "test".to_string(),
                    components,
                }),
                A2uiMessage::UpdateDataModel(UpdateDataModel {
                    surface_id: "test".to_string(),
                    path: None,
                    value: data,
                }),
            ])
            .unwrap();
        store.surface("test").unwrap().clone()
    }

    #[test]
    fn renders_column_row_card_badge_and_divider() {
        let surface = build_surface(
            vec![
                A2uiComponent::new(
                    "root",
                    A2uiComponentKind::Card {
                        title: Some(BoundString::literal("Status")),
                        child: "col".to_string(),
                        tone: Some(SemanticTone::Accent),
                    },
                ),
                A2uiComponent::new(
                    "col",
                    A2uiComponentKind::Column {
                        children: vec!["hdr".to_string(), "div".to_string(), "body".to_string()],
                        gap: Some(0),
                    },
                ),
                A2uiComponent::new(
                    "hdr",
                    A2uiComponentKind::Row {
                        children: vec!["title".to_string(), "badge".to_string()],
                        gap: Some(1),
                        justify: Some(RowJustify::SpaceBetween),
                    },
                ),
                A2uiComponent::new(
                    "title",
                    A2uiComponentKind::Text {
                        text: BoundString::literal("Tasks"),
                        usage_hint: TextUsageHint::H3,
                        tone: None,
                    },
                ),
                A2uiComponent::new(
                    "badge",
                    A2uiComponentKind::Badge {
                        label: BoundString::literal("1/2"),
                        tone: SemanticTone::Success,
                    },
                ),
                A2uiComponent::new(
                    "div",
                    A2uiComponentKind::Divider {
                        label: Some(BoundString::literal("items")),
                    },
                ),
                A2uiComponent::new(
                    "body",
                    A2uiComponentKind::Text {
                        text: BoundString::path("/msg"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                ),
            ],
            json!({ "msg": "Working" }),
        );

        let lines = render_surface(&surface, 40, &Theme::default());
        assert_eq!(lines.len(), 5);
        for line in &lines {
            assert_eq!(visible_width(line), 40, "line: {:?}", strip_ansi(line));
        }
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert!(plain[0].starts_with("╭─ Status "));
        assert!(plain[1].contains("Tasks") && plain[1].contains("[1/2]"));
        assert!(plain[2].contains("── items ─"));
        assert!(plain[3].contains("Working"));
        assert!(plain[4].starts_with("╰") && plain[4].ends_with("╯"));
    }

    #[test]
    fn renders_table_and_clamps_to_width() {
        let surface = build_surface(
            vec![A2uiComponent::new(
                "root",
                A2uiComponentKind::Table {
                    headers: vec![BoundString::literal("Name"), BoundString::literal("State")],
                    rows: BoundValue::Path("/rows".to_string()),
                },
            )],
            json!({
                "rows": [
                    ["alpha-very-long-name", "running"],
                    ["beta", "done"],
                ]
            }),
        );

        let lines = render_surface(&surface, 20, &Theme::default());
        assert_eq!(lines.len(), 4);
        for line in &lines {
            assert!(visible_width(line) <= 20, "exceeded width: {}", strip_ansi(line));
        }
        assert!(strip_ansi(&lines[1]).contains('┼'));
    }

    #[test]
    fn truncates_surfaces_at_max_lines_with_dimmed_notice() {
        let items = vec![
            TodoItem {
                content: "First".to_string(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                content: "Second".to_string(),
                status: TodoStatus::Pending,
            },
            TodoItem {
                content: "Third".to_string(),
                status: TodoStatus::Pending,
            },
        ];
        let mut store = SurfaceStore::new();
        store.apply_all(todo_surface_messages(&items)).unwrap();
        let surface = store.surface(rho_harness_core::a2ui::TODO_SURFACE_ID).unwrap();

        let full = render_surface(surface, 50, &Theme::default());
        assert!(full.len() >= 5);

        let truncated = render_surfaces(&[surface], 50, 3, &Theme::default());
        assert_eq!(truncated.len(), 3);
        let last_plain = strip_ansi(&truncated[2]);
        assert!(last_plain.starts_with("... (+"), "got {last_plain:?}");
        assert!(last_plain.ends_with("lines)"));
    }

    #[test]
    fn masked_text_field_caps_asterisks_at_twelve() {
        let surface = build_surface(
            vec![A2uiComponent::new(
                "root",
                A2uiComponentKind::TextField {
                    label: None,
                    placeholder: None,
                    value_path: "/pw".to_string(),
                    multiline: false,
                    masked: true,
                },
            )],
            json!({ "pw": "x".repeat(20) }),
        );
        let lines = render_surface(&surface, 60, &Theme::default());
        assert_eq!(strip_ansi(&lines[0]), format!("[{}]", "*".repeat(12)));
    }

    #[test]
    fn code_blocks_replace_tabs_before_rendering() {
        let surface = build_surface(
            vec![A2uiComponent::new(
                "root",
                A2uiComponentKind::Code {
                    code: BoundString::literal("a\tb"),
                    language: None,
                },
            )],
            json!({}),
        );
        let lines = render_surface(&surface, 40, &Theme::default());
        assert!(
            !strip_ansi(&lines[0]).contains('\t'),
            "got: {:?}",
            strip_ansi(&lines[0])
        );
    }

    #[test]
    fn component_shared_by_two_parents_renders_under_each() {
        let surface = build_surface(
            vec![
                A2uiComponent::new(
                    "root",
                    A2uiComponentKind::Column {
                        children: vec!["shared".to_string(), "shared".to_string()],
                        gap: None,
                    },
                ),
                A2uiComponent::new(
                    "shared",
                    A2uiComponentKind::Text {
                        text: BoundString::literal("hi"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                ),
            ],
            json!({}),
        );
        let lines = render_surface(&surface, 40, &Theme::default());
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn handles_cycles_without_infinite_recursion() {
        let surface = build_surface(
            vec![
                A2uiComponent::new(
                    "root",
                    A2uiComponentKind::Column {
                        children: vec!["a".to_string()],
                        gap: None,
                    },
                ),
                A2uiComponent::new(
                    "a",
                    A2uiComponentKind::Column {
                        children: vec!["root".to_string(), "b".to_string()],
                        gap: None,
                    },
                ),
                A2uiComponent::new(
                    "b",
                    A2uiComponentKind::Text {
                        text: BoundString::literal("leaf"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                ),
            ],
            json!({}),
        );
        let lines = render_surface(&surface, 30, &Theme::default());
        assert_eq!(lines, vec!["leaf"]);
    }
}
