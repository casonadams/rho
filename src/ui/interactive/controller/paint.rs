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

fn paint_diff_lines<B: TerminalBackend>(
    backend: &mut B,
    (lines, prev_height): (&[String], usize),
    bg: &str,
) -> io::Result<()> {
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            if i < prev_height {
                backend.move_to_column(0)?;
                backend.move_down(1)?;
            } else {
                backend.write_text("\r\n")?;
            }
        }
        clear_line(backend, bg)?;
        backend.write_text(line)?;
    }
    Ok(())
}

fn clear_excess_lines<B: TerminalBackend>(
    backend: &mut B,
    (prev_height, new_height): (usize, usize),
    bg: &str,
) -> io::Result<()> {
    for _ in new_height..prev_height {
        backend.move_to_column(0)?;
        backend.move_down(1)?;
        clear_line(backend, bg)?;
    }
    Ok(())
}

fn move_cursor_to_target<B: TerminalBackend>(
    backend: &mut B,
    base_height: usize,
    layout: &InteractiveLayout,
) -> io::Result<()> {
    let rows_up = base_height.saturating_sub(1).saturating_sub(layout.cursor_row());
    if rows_up > 0 {
        backend.move_to_column(0)?;
        backend.move_up(rows_up)?;
    }
    backend.move_to_column(layout.cursor.column)
}

fn render_diff_with_prev<B: TerminalBackend>(
    backend: &mut B,
    prev: &InteractiveLayout,
    next: &InteractiveLayout,
) -> io::Result<()> {
    if prev.cursor_row() > 0 {
        backend.move_up(prev.cursor_row())?;
    }
    backend.move_to_column(0)?;
    paint_diff_lines(backend, (&next.lines, prev.height()), &next.bg)?;
    clear_excess_lines(backend, (prev.height(), next.lines.len()), &next.bg)?;
    move_cursor_to_target(backend, prev.height().max(next.lines.len()), next)
}

fn render_initial_layout<B: TerminalBackend>(backend: &mut B, next: &InteractiveLayout) -> io::Result<()> {
    for (i, line) in next.lines.iter().enumerate() {
        if i > 0 {
            backend.write_text("\r\n")?;
        }
        backend.write_text(line)?;
    }
    move_cursor_to_target(backend, next.lines.len(), next)
}

fn apply_cursor_visibility<B: TerminalBackend>(backend: &mut B, visible: bool) -> io::Result<()> {
    if visible {
        backend.show_cursor()
    } else {
        backend.hide_cursor()
    }
}

pub fn render_live_diff<B: TerminalBackend>(
    backend: &mut B,
    prev: Option<&InteractiveLayout>,
    next: &InteractiveLayout,
) -> io::Result<()> {
    backend.write_text(CSI_BEGIN_SYNC_UPDATE)?;
    backend.hide_cursor()?;
    if let Some(prev) = prev {
        render_diff_with_prev(backend, prev, next)?;
    } else {
        render_initial_layout(backend, next)?;
    }
    apply_cursor_visibility(backend, next.cursor_visible)?;
    backend.write_text(CSI_END_SYNC_UPDATE)?;
    backend.flush()
}
