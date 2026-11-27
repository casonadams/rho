use std::collections::HashSet;

use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, BoundString, ButtonVariant, MAX_DEPTH, RowJustify, SemanticTone,
};

use super::A2uiModalState;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{
    card_border_style, card_frame, card_inner_width, clamp_line, layout_row_segments, render_badge, render_code,
    render_divider, render_table, render_text, style_line,
};
use crate::ui::theme::Theme;

mod choice;
mod text;
mod viewport;

use choice::{ChoicePickerRender, render_modal_checkbox, render_modal_choice_picker};
use text::render_modal_text_field;
use viewport::{adjust_cursor, window_around_focus};

impl A2uiModalState {
    pub fn desired_lines(&self, width: usize) -> usize {
        self.render_tree(width.max(1), &Theme::default(), false)
            .lines
            .len()
            .max(1)
    }

    pub fn render(
        &self,
        width: usize,
        max_lines: usize,
        theme: &Theme,
        focused: bool,
    ) -> (Vec<String>, CursorPosition, bool) {
        if width == 0 || max_lines == 0 {
            return (Vec::new(), CursorPosition { row: 0, column: 0 }, false);
        }
        let tree = self.render_tree(width, theme, focused);
        let lines: Vec<String> = tree.lines.into_iter().map(|l| clamp_line(&l, width)).collect();
        let (windowed, offset) = window_around_focus(lines, tree.focus_range, max_lines, width, theme);
        let (mut cursor, cursor_visible) = adjust_cursor(tree.cursor, offset, windowed.len(), focused);
        let indented = windowed.into_iter().map(|l| format!("  {l}")).collect();
        if cursor_visible {
            cursor.column += 2;
        }
        (indented, cursor, cursor_visible)
    }

    fn render_tree(&self, width: usize, theme: &Theme, focused: bool) -> RenderedTree {
        let mut visited = HashSet::new();
        let mut ctx = ModalRenderCtx {
            state: self,
            depth: 0,
            visited: &mut visited,
            width,
            theme,
            focused,
            cursor: None,
            focus_range: None,
        };
        let lines = render_modal_node(&mut ctx, &self.surface.root_id);
        RenderedTree {
            lines,
            cursor: ctx.cursor,
            focus_range: ctx.focus_range,
        }
    }
}

struct RenderedTree {
    lines: Vec<String>,
    cursor: Option<CursorPosition>,
    focus_range: Option<(usize, usize)>,
}

struct ModalRenderCtx<'a, 'b> {
    state: &'a A2uiModalState,
    depth: usize,
    visited: &'b mut HashSet<&'a str>,
    width: usize,
    theme: &'a Theme,
    focused: bool,
    cursor: Option<CursorPosition>,
    focus_range: Option<(usize, usize)>,
}

impl<'a> ModalRenderCtx<'a, '_> {
    fn child(&mut self, width: usize) -> ModalRenderCtx<'a, '_> {
        ModalRenderCtx {
            state: self.state,
            depth: self.depth + 1,
            visited: self.visited,
            width,
            theme: self.theme,
            focused: self.focused,
            cursor: None,
            focus_range: None,
        }
    }
}

fn render_modal_node<'a>(ctx: &mut ModalRenderCtx<'a, '_>, id: &'a str) -> Vec<String> {
    if ctx.depth >= MAX_DEPTH || ctx.width == 0 || !ctx.visited.insert(id) {
        return Vec::new();
    }
    let Some(component) = ctx.state.surface.component(id) else {
        return Vec::new();
    };
    render_modal_component(ctx, component)
}

fn render_modal_component<'a>(ctx: &mut ModalRenderCtx<'a, '_>, component: &'a A2uiComponent) -> Vec<String> {
    let surface = &ctx.state.surface;
    match &component.kind {
        A2uiComponentKind::Column { children, gap } => render_modal_column(ctx, children, gap.unwrap_or(0) as usize),
        A2uiComponentKind::Row { children, gap, justify } => {
            render_modal_row(ctx, children, gap.unwrap_or(1) as usize, justify.unwrap_or_default())
        }
        A2uiComponentKind::Card { title, child, tone } => render_modal_card(ctx, title.as_ref(), child, *tone),
        A2uiComponentKind::Divider { label } => render_divider(surface, label.as_ref(), ctx.width, ctx.theme),
        A2uiComponentKind::Text { text, usage_hint, tone } => {
            render_text(surface, text, *usage_hint, *tone, ctx.width, ctx.theme)
        }
        A2uiComponentKind::Code { code, language } => {
            render_code(surface, code, language.as_deref(), ctx.width, ctx.theme)
        }
        A2uiComponentKind::Badge { label, tone } => render_badge(surface, label, *tone, ctx.width, ctx.theme),
        A2uiComponentKind::Table { headers, rows } => render_table(surface, headers, rows, ctx.width, ctx.theme),
        A2uiComponentKind::Button { label, variant, .. } => render_modal_button(ctx, &component.id, label, *variant),
        A2uiComponentKind::ChoicePicker {
            label,
            options,
            selection_path,
            multi_select,
            allow_other,
            other_path,
        } => render_modal_choice_picker(
            ctx,
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
        A2uiComponentKind::TextField {
            label,
            placeholder,
            value_path,
            masked,
            ..
        } => render_modal_text_field(
            ctx,
            &component.id,
            label.as_ref(),
            placeholder.as_deref(),
            value_path,
            *masked,
        ),
        A2uiComponentKind::CheckBox { label, checked_path } => {
            render_modal_checkbox(ctx, &component.id, label, checked_path)
        }
    }
}

fn merge_child_offsets(
    ctx: &mut ModalRenderCtx<'_, '_>,
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

fn render_modal_column<'a>(ctx: &mut ModalRenderCtx<'a, '_>, children: &'a [String], gap: usize) -> Vec<String> {
    let mut out = Vec::new();
    let width = ctx.width;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let rendered = render_modal_node(&mut child_ctx, child_id);
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

fn render_modal_row<'a>(
    ctx: &mut ModalRenderCtx<'a, '_>,
    children: &'a [String],
    gap: usize,
    justify: RowJustify,
) -> Vec<String> {
    let mut segments = Vec::new();
    let width = ctx.width;
    let mut has_focus = false;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let lines = render_modal_node(&mut child_ctx, child_id);
        if !lines.is_empty() {
            has_focus |= child_ctx.focus_range.is_some();
            segments.push(lines.join(" "));
        }
    }
    if segments.is_empty() {
        return Vec::new();
    }
    if has_focus {
        ctx.focus_range = Some((0, 1));
    }
    vec![layout_row_segments(&segments, gap, justify, width)]
}

fn render_modal_card<'a>(
    ctx: &mut ModalRenderCtx<'a, '_>,
    title: Option<&BoundString>,
    child: &'a str,
    tone: Option<SemanticTone>,
) -> Vec<String> {
    let width = ctx.width;
    if width < 4 {
        let mut child_ctx = ctx.child(width);
        let out = render_modal_node(&mut child_ctx, child);
        let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
        merge_child_offsets(ctx, child_cursor, child_focus, 0, 0);
        return out;
    }
    let mut child_ctx = ctx.child(card_inner_width(width));
    let child_lines = render_modal_node(&mut child_ctx, child);
    let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
    merge_child_offsets(ctx, child_cursor, child_focus, 1, 2);
    let border = card_border_style(tone, ctx.theme);
    card_frame(&ctx.state.surface, title, width, border, child_lines)
}

fn render_modal_button(
    ctx: &mut ModalRenderCtx<'_, '_>,
    id: &str,
    label: &BoundString,
    variant: ButtonVariant,
) -> Vec<String> {
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let resolved = ctx.state.surface.resolve_string(label);
    let style = match (is_focused, variant) {
        (true, ButtonVariant::Danger) => ctx.theme.tool_err.bold(),
        (true, _) => ctx.theme.highlight.bold(),
        (false, ButtonVariant::Primary) => ctx.theme.highlight,
        (false, ButtonVariant::Secondary) => ctx.theme.dimmed,
        (false, ButtonVariant::Danger) => ctx.theme.tool_err,
    };
    let text = if is_focused {
        format!("▸ [ {resolved} ]")
    } else {
        format!("[ {resolved} ]")
    };
    if is_focused {
        ctx.focus_range = Some((0, 1));
    }
    vec![clamp_line(&style_line(&text, style), ctx.width)]
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::{create_surface, key, single_select_request};
    use super::super::A2uiModalState;
    use crate::ui::block::{strip_ansi, visible_width};
    use crate::ui::theme::Theme;
    use crossterm::event::KeyCode;
    use rho_harness_core::a2ui::{
        A2uiComponent, A2uiComponentKind, A2uiMessage, A2uiModalRequest, BoundString, ChoiceOption, UpdateComponents,
    };

    #[test]
    fn other_option_renders_ellipsis_when_idle_and_box_when_active() {
        let mut state = A2uiModalState::from_request(single_select_request(true)).unwrap();
        let (lines, _, _) = state.render(60, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let other_line = plain.iter().find(|l| l.contains("Other")).expect("other line");
        assert!(other_line.contains("○ Other…"), "got: {other_line}");
        assert!(!other_line.contains("Other: []"));

        state.handle_key(key(KeyCode::Char('3')));
        let (lines, _, cursor_vis) = state.render(60, 10, &Theme::default(), true);
        assert!(cursor_vis);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let other_line = plain.iter().find(|l| l.contains("Other")).expect("other line");
        assert!(other_line.contains("Other: ["), "got: {other_line}");
    }

    #[test]
    fn choice_descriptions_align_vertically_and_truncate_with_ellipsis() {
        let req = A2uiModalRequest {
            surface_id: "align".to_string(),
            title: Some("Align".to_string()),
            messages: vec![
                create_surface("align", "q1", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "align".to_string(),
                    components: vec![A2uiComponent::new(
                        "q1",
                        A2uiComponentKind::ChoicePicker {
                            label: None,
                            options: vec![
                                ChoiceOption {
                                    value: "0".to_string(),
                                    label: BoundString::literal("Go"),
                                    description: Some(BoundString::literal("Short desc")),
                                },
                                ChoiceOption {
                                    value: "1".to_string(),
                                    label: BoundString::literal("PostgreSQL"),
                                    description: Some(BoundString::literal(
                                        "Very long description that should truncate cleanly",
                                    )),
                                },
                            ],
                            selection_path: "/q1".to_string(),
                            multi_select: false,
                            allow_other: false,
                            other_path: None,
                        },
                    )],
                }),
            ],
            timeout_ms: None,
        };
        let state = A2uiModalState::from_request(req).unwrap();
        let (lines, _, _) = state.render(44, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let col0 = plain[0].find("Short desc").expect("first desc");
        let col1 = plain[1].find("Very long").expect("second desc");
        assert_eq!(
            visible_width(&plain[0][..col0]),
            visible_width(&plain[1][..col1]),
            "descriptions should align vertically: {plain:?}"
        );
        assert!(plain[1].ends_with('…'), "expected ellipsis truncation: {:?}", plain[1]);
    }
}
