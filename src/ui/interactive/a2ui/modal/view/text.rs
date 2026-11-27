use rho_harness_core::a2ui::BoundString;

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
    let display = if val.is_empty() && !is_focused {
        let ph = placeholder.unwrap_or("");
        format!("{}{ph}{:#}", ctx.theme.dimmed, ctx.theme.dimmed)
    } else if masked {
        mask_text(&val, None)
    } else {
        val
    };
    let head = format!("{}[", option_prefix(ctx.theme, is_focused));
    let col = (visible_width(&head) + visible_width(&display)).min(ctx.width);
    let cursor_block = software_cursor(ctx.theme, is_focused && modal.focused);
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
