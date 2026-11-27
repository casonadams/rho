use rho_harness_core::a2ui::{BoundString, ChoiceOption, resolve_pointer};
use serde_json::Value;

use super::super::controls::OTHER_SENTINEL;
use super::super::options::{
    ChoiceMark, ChoiceOptionFormat, OtherOptionRender, format_choice_option_line, format_other_option_line,
    option_prefix,
};
use crate::ui::block::visible_width;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{ModalFocus, RenderCtx, clamp_line, render_optional_label, style_line};
use crate::ui::theme::SGR_RESET;

pub(in crate::ui::interactive::a2ui) struct ChoicePickerRender<'a> {
    pub id: &'a str,
    pub label: Option<&'a BoundString>,
    pub options: &'a [ChoiceOption],
    pub selection_path: &'a str,
    pub multi_select: bool,
    pub allow_other: bool,
    pub other_path: Option<&'a str>,
}

struct ChoiceSelection {
    multi_select: bool,
    single: Option<String>,
    multi: Vec<String>,
}

impl ChoiceSelection {
    fn read(modal: ModalFocus<'_>, selection_path: &str, multi_select: bool) -> Self {
        Self {
            multi_select,
            single: resolve_pointer(&modal.state.surface.data_model, selection_path)
                .and_then(Value::as_str)
                .map(str::to_owned),
            multi: modal.state.read_string_array(selection_path),
        }
    }

    fn mark(&self, value: &str) -> ChoiceMark {
        if self.multi_select {
            ChoiceMark::Checkbox(self.multi.iter().any(|v| v == value))
        } else {
            ChoiceMark::Radio(self.single.as_deref() == Some(value))
        }
    }
}

fn label_column_width(ctx: &RenderCtx<'_, '_>, options: &[ChoiceOption]) -> usize {
    options
        .iter()
        .map(|opt| visible_width(&ctx.surface.resolve_string(&opt.label)))
        .max()
        .unwrap_or(0)
        .min((ctx.width / 2).max(1))
}

pub(in crate::ui::interactive::a2ui) fn render_modal_choice_picker(
    ctx: &mut RenderCtx<'_, '_>,
    modal: ModalFocus<'_>,
    spec: ChoicePickerRender<'_>,
) -> Vec<String> {
    let ChoicePickerRender {
        id,
        label,
        options,
        selection_path,
        multi_select,
        allow_other,
        other_path,
    } = spec;
    let is_focused = modal.state.focused_component_id() == Some(id);
    let cursor = modal.state.choice_cursor(id);
    let selection = ChoiceSelection::read(modal, selection_path, multi_select);
    let mut out = render_optional_label(ctx.surface, label, ctx.width, ctx.theme);
    let label_col_w = label_column_width(ctx, options);

    for (idx, opt) in options.iter().enumerate() {
        let line = format_choice_option_line(ChoiceOptionFormat {
            surface: ctx.surface,
            opt,
            active_cursor: is_focused && cursor == idx,
            mark: selection.mark(&opt.value),
            label_col_w,
            max_width: ctx.width,
            theme: ctx.theme,
        });
        out.push(clamp_line(&line, ctx.width));
    }

    if allow_other {
        let other_text = modal.state.read_string(other_path.unwrap_or(selection_path));
        let editing = is_focused && modal.state.editing_other;
        push_other_choice(
            ctx,
            modal,
            &mut out,
            OtherChoice {
                active_cursor: is_focused && cursor == options.len(),
                mark: selection.mark(OTHER_SENTINEL),
                other_text: &other_text,
                editing,
            },
        );
    }

    if is_focused {
        ctx.focus_range = Some((0, out.len().max(1)));
    }
    out
}

struct OtherChoice<'a> {
    active_cursor: bool,
    mark: ChoiceMark,
    other_text: &'a str,
    editing: bool,
}

fn push_other_choice(
    ctx: &mut RenderCtx<'_, '_>,
    modal: ModalFocus<'_>,
    out: &mut Vec<String>,
    other: OtherChoice<'_>,
) {
    let (line, col) = format_other_option_line(OtherOptionRender {
        active_cursor: other.active_cursor,
        mark: other.mark,
        other_text: other.other_text,
        editing: other.editing,
        focused: modal.focused,
        theme: ctx.theme,
    });
    if other.editing {
        ctx.cursor = Some(CursorPosition {
            row: out.len(),
            column: col.min(ctx.width),
        });
    }
    out.push(clamp_line(&line, ctx.width));
}

pub(in crate::ui::interactive::a2ui) fn render_modal_checkbox(
    ctx: &mut RenderCtx<'_, '_>,
    modal: ModalFocus<'_>,
    id: &str,
    label: &BoundString,
    checked_path: &str,
) -> Vec<String> {
    let is_focused = modal.state.focused_component_id() == Some(id);
    let checked = resolve_pointer(&ctx.surface.data_model, checked_path)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mark = if checked { "[x]" } else { "[ ]" };
    let text = ctx.surface.resolve_string(label);
    let line = format!("{}{mark} {text}", option_prefix(ctx.theme, is_focused));
    let styled = if checked {
        style_line(&line, ctx.theme.tool_ok)
    } else if is_focused {
        format!("\x1b[1m{line}{SGR_RESET}")
    } else {
        line
    };
    if is_focused {
        ctx.focus_range = Some((0, 1));
    }
    vec![clamp_line(&styled, ctx.width)]
}
