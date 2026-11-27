use rho_harness_core::a2ui::BoundString;

use super::super::options::{option_prefix, software_cursor};
use super::ModalRenderCtx;
use crate::ui::block::visible_width;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{clamp_line, render_optional_label};

pub(super) fn render_modal_text_field(
    ctx: &mut ModalRenderCtx<'_, '_>,
    id: &str,
    label: Option<&BoundString>,
    placeholder: Option<&str>,
    value_path: &str,
    masked: bool,
) -> Vec<String> {
    let mut out = render_optional_label(&ctx.state.surface, label, ctx.width, ctx.theme);
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let val = ctx.state.read_string(value_path);
    let display = if val.is_empty() && !is_focused {
        let ph = placeholder.unwrap_or("");
        format!("{}{ph}{:#}", ctx.theme.dimmed, ctx.theme.dimmed)
    } else if masked {
        "*".repeat(val.chars().count())
    } else {
        val
    };
    let head = format!("{}[", option_prefix(ctx.theme, is_focused));
    let col = (visible_width(&head) + visible_width(&display)).min(ctx.width);
    let cursor_block = software_cursor(ctx.theme, is_focused && ctx.focused);
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
