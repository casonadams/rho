use std::io;

use super::ansi::{CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE};
use super::backend::TerminalBackend;
use crate::ui::interactive::InteractiveLayout;

fn clear_line<B: TerminalBackend>(backend: &mut B, bg: &str) -> io::Result<()> {
    if !bg.is_empty() {
        backend.write_text(bg)?;
    }
    backend.clear_line()
}

pub fn write_live_region<B: TerminalBackend>(backend: &mut B, rendered: &InteractiveLayout) -> io::Result<()> {
    let total = rendered.lines.len();
    for (i, line) in rendered.lines.iter().enumerate() {
        backend.write_text(line)?;
        if i + 1 < total {
            backend.write_text("\r\n")?;
        }
    }
    let rows_up = (total.saturating_sub(1)).saturating_sub(rendered.cursor_row());
    if rows_up > 0 {
        backend.move_to_column(0)?;
        backend.move_up(rows_up)?;
    }
    backend.move_to_column(rendered.cursor.column)
}

pub fn erase_live_region<B: TerminalBackend>(
    backend: &mut B,
    rendered: Option<&InteractiveLayout>,
    bg: &str,
) -> io::Result<()> {
    let Some(rendered) = rendered else {
        return Ok(());
    };
    let height = rendered.height();
    let cursor_row = rendered.cursor_row();
    if height > 0 {
        let rows_down = (height.saturating_sub(1)).saturating_sub(cursor_row);
        backend.move_to_column(0)?;
        if rows_down > 0 {
            backend.move_down(rows_down)?;
        }
        for row in (0..height).rev() {
            clear_line(backend, bg)?;
            if row > 0 {
                backend.move_up(1)?;
            }
        }
        backend.move_to_column(0)?;
    }
    Ok(())
}

pub fn render_live_diff<B: TerminalBackend>(
    backend: &mut B,
    prev: Option<&InteractiveLayout>,
    next: &InteractiveLayout,
) -> io::Result<()> {
    let next_lines = &next.lines;
    let new_height = next_lines.len();
    let target_cursor_row = next.cursor_row();
    let target_cursor_col = next.cursor.column;

    backend.write_text(CSI_BEGIN_SYNC_UPDATE)?;
    backend.hide_cursor()?;

    if let Some(prev) = prev {
        let prev_height = prev.height();
        let prev_cursor_row = prev.cursor_row();

        if prev_cursor_row > 0 {
            backend.move_up(prev_cursor_row)?;
        }
        backend.move_to_column(0)?;

        for (i, line) in next_lines.iter().enumerate() {
            if i > 0 {
                if i < prev_height {
                    backend.move_to_column(0)?;
                    backend.move_down(1)?;
                } else {
                    backend.write_text("\r\n")?;
                }
            }
            clear_line(backend, &next.bg)?;
            backend.write_text(line)?;
        }

        if prev_height > new_height {
            for _ in new_height..prev_height {
                backend.move_to_column(0)?;
                backend.move_down(1)?;
                clear_line(backend, &next.bg)?;
            }
        }
        let base_height = prev_height.max(new_height);
        let rows_up = (base_height.saturating_sub(1)).saturating_sub(target_cursor_row);
        if rows_up > 0 {
            backend.move_to_column(0)?;
            backend.move_up(rows_up)?;
        }
        backend.move_to_column(target_cursor_col)?;
    } else {
        for (i, line) in next_lines.iter().enumerate() {
            if i > 0 {
                backend.write_text("\r\n")?;
            }
            backend.write_text(line)?;
        }
        let rows_up = (new_height.saturating_sub(1)).saturating_sub(target_cursor_row);
        if rows_up > 0 {
            backend.move_to_column(0)?;
            backend.move_up(rows_up)?;
        }
        backend.move_to_column(target_cursor_col)?;
    }

    if next.cursor_visible {
        backend.show_cursor()?;
    } else {
        backend.hide_cursor()?;
    }
    backend.write_text(CSI_END_SYNC_UPDATE)?;
    backend.flush()
}
