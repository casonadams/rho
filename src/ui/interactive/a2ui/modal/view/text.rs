use rho_harness_core::a2ui::BoundString;
use unicode_width::UnicodeWidthChar;

use super::super::options::{option_prefix, software_cursor};
use crate::ui::block::visible_width;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{ModalFocus, RenderCtx, clamp_line, mask_text, render_optional_label};

pub(in crate::ui::interactive::a2ui) struct TextFieldRender<'a> {
    pub id: &'a str,
    pub label: Option<&'a BoundString>,
    pub placeholder: Option<&'a str>,
    pub value_path: &'a str,
    pub masked: bool,
}

pub(in crate::ui::interactive::a2ui) fn render_modal_text_field(
    ctx: &mut RenderCtx<'_, '_>,
    modal: ModalFocus<'_>,
    field: TextFieldRender<'_>,
) -> Vec<String> {
    let TextFieldRender {
        id,
        label,
        placeholder,
        value_path,
        masked,
    } = field;
    let mut out = render_optional_label(ctx.surface, label, ctx.width, ctx.theme);
    let is_focused = modal.state.focused_component_id() == Some(id);
    let val = modal.state.read_string(value_path);
    let head = format!("{}[", option_prefix(ctx.theme, is_focused));
    let cursor_block = software_cursor(ctx.theme, is_focused && modal.focused);
    let budget = ctx
        .width
        .saturating_sub(visible_width(&head) + visible_width(cursor_block) + 1);
    let display = if val.is_empty() && !is_focused {
        let ph = placeholder.unwrap_or("");
        format!("{}{ph}{:#}", ctx.theme.dimmed, ctx.theme.dimmed)
    } else {
        let shown = if masked { mask_text(&val, None) } else { val };
        tail_within_width(&shown, budget).to_string()
    };
    let col = (visible_width(&head) + visible_width(&display)).min(ctx.width);
    let input_row = out.len();
    out.push(clamp_line(&format!("{head}{display}{cursor_block}]"), ctx.width));
    if is_focused {
        ctx.cursor = Some(CursorPosition {
            row: input_row,
            column: col,
        });
        ctx.focus_range = Some((0, out.len().max(1)));
    }
    out
}

/// The longest suffix of `text` whose visible width fits in `width`; the typing cursor sits at the end.
fn tail_within_width(text: &str, width: usize) -> &str {
    let mut used = 0;
    let mut start = text.len();
    for (idx, c) in text.char_indices().rev() {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + char_width > width {
            break;
        }
        used += char_width;
        start = idx;
    }
    &text[start..]
}
