use std::io::{self, Stdout, Write};

use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    queue,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};

use super::{InteractiveLayout, InteractiveState, LayoutInput, layout};

pub trait TerminalBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()>;
    fn size(&self) -> io::Result<(u16, u16)>;
    fn hide_cursor(&mut self) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
    fn move_up(&mut self, rows: usize) -> io::Result<()>;
    fn move_down(&mut self, rows: usize) -> io::Result<()>;
    fn move_to_column(&mut self, column: usize) -> io::Result<()>;
    fn clear_line(&mut self) -> io::Result<()>;
    fn write_text(&mut self, text: &str) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}

pub struct CrosstermBackend {
    stdout: Stdout,
}

impl CrosstermBackend {
    pub fn stdout() -> Self {
        Self { stdout: io::stdout() }
    }
}

impl TerminalBackend for CrosstermBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        if enabled { enable_raw_mode() } else { disable_raw_mode() }
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        size()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout, Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout, Show)
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        queue_vertical_move(&mut self.stdout, rows, true)
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        queue_vertical_move(&mut self.stdout, rows, false)
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        queue!(self.stdout, MoveToColumn(column.min(u16::MAX as usize) as u16))
    }

    fn clear_line(&mut self) -> io::Result<()> {
        queue!(self.stdout, Clear(ClearType::CurrentLine))
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.stdout.write_all(text.as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

fn queue_vertical_move(stdout: &mut Stdout, mut rows: usize, upward: bool) -> io::Result<()> {
    while rows > 0 {
        let step = rows.min(u16::MAX as usize) as u16;
        if upward {
            queue!(stdout, MoveUp(step))?;
        } else {
            queue!(stdout, MoveDown(step))?;
        }
        rows -= usize::from(step);
    }
    Ok(())
}

pub struct TerminalController<B: TerminalBackend> {
    backend: B,
    state: InteractiveState,
    width: usize,
    rendered: Option<InteractiveLayout>,
    active: bool,
}

impl TerminalController<CrosstermBackend> {
    pub fn stdout(state: InteractiveState) -> io::Result<Self> {
        Self::new(CrosstermBackend::stdout(), state)
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(mut backend: B, state: InteractiveState) -> io::Result<Self> {
        backend.set_raw_mode(true)?;
        if let Err(error) = backend.hide_cursor() {
            let _ = backend.set_raw_mode(false);
            return Err(error);
        }
        let width = match backend.size() {
            Ok((width, _)) => usize::from(width),
            Err(error) => {
                let _ = backend.show_cursor();
                let _ = backend.set_raw_mode(false);
                return Err(error);
            }
        };
        let mut controller = Self {
            backend,
            state,
            width,
            rendered: None,
            active: true,
        };
        if let Err(error) = controller.redraw() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    pub fn state(&self) -> &InteractiveState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut InteractiveState {
        &mut self.state
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        self.erase_live_region()?;
        let rendered = layout(LayoutInput {
            editor: self.state.editor(),
            footer: self.state.footer(),
            queued_messages: self.state.queue_len(),
            terminal_width: self.width,
        });
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.flush()
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        self.erase_live_region()?;
        let output = terminal_newlines(output);
        self.backend.write_text(&output)?;
        if !output.ends_with("\r\n") {
            self.backend.write_text("\r\n")?;
        }
        let rendered = layout(LayoutInput {
            editor: self.state.editor(),
            footer: self.state.footer(),
            queued_messages: self.state.queue_len(),
            terminal_width: self.width,
        });
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.flush()
    }

    pub fn refresh_size(&mut self) -> io::Result<bool> {
        let (width, _) = self.backend.size()?;
        let width = usize::from(width);
        if width == self.width {
            return Ok(false);
        }
        self.width = width;
        self.redraw()?;
        Ok(true)
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.redraw()
    }

    fn write_live_region(&mut self, rendered: &InteractiveLayout) -> io::Result<()> {
        self.backend.write_text(&rendered.top_divider)?;
        self.backend.write_text("\r\n")?;
        for line in &rendered.editor_lines {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
        }
        self.backend.write_text(&rendered.bottom_divider)?;
        self.backend.write_text("\r\n")?;
        self.backend.write_text(&rendered.footer)?;

        let cursor_row = rendered.cursor.row + 1;
        self.backend.move_up(rendered.height() - 1 - cursor_row)?;
        self.backend.move_to_column(rendered.cursor.column)
    }

    fn erase_live_region(&mut self) -> io::Result<()> {
        let Some(rendered) = self.rendered.as_ref() else {
            return Ok(());
        };
        let cursor_row = rendered.cursor.row + 1;
        self.backend.move_down(rendered.height() - 1 - cursor_row)?;
        self.backend.move_to_column(0)?;
        for row in (0..rendered.height()).rev() {
            self.backend.clear_line()?;
            if row > 0 {
                self.backend.move_up(1)?;
            }
        }
        self.backend.move_to_column(0)
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.erase_live_region();
        let _ = self.backend.show_cursor();
        let _ = self.backend.set_raw_mode(false);
        let _ = self.backend.flush();
        self.active = false;
    }
}

impl<B: TerminalBackend> Drop for TerminalController<B> {
    fn drop(&mut self) {
        self.restore();
    }
}

fn terminal_newlines(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_carriage_return = false;
    for character in value.chars() {
        if character == '\n' && !previous_was_carriage_return {
            result.push('\r');
        }
        result.push(character);
        previous_was_carriage_return = character == '\r';
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io,
        rc::Rc,
    };

    use super::{TerminalBackend, TerminalController};
    use crate::ui::interactive::InteractiveState;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Operation {
        Raw(bool),
        Size,
        Hide,
        Show,
        Up(usize),
        Down(usize),
        Column(usize),
        Clear,
        Write(String),
        Flush,
    }

    struct FakeTerminal {
        operations: Rc<RefCell<Vec<Operation>>>,
        width: Rc<Cell<u16>>,
        fail_write: bool,
    }

    impl FakeTerminal {
        fn new(width: u16) -> (Self, Rc<RefCell<Vec<Operation>>>, Rc<Cell<u16>>) {
            let operations = Rc::new(RefCell::new(Vec::new()));
            let width = Rc::new(Cell::new(width));
            (
                Self {
                    operations: Rc::clone(&operations),
                    width: Rc::clone(&width),
                    fail_write: false,
                },
                operations,
                width,
            )
        }
    }

    impl TerminalBackend for FakeTerminal {
        fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Raw(enabled));
            Ok(())
        }

        fn size(&self) -> io::Result<(u16, u16)> {
            self.operations.borrow_mut().push(Operation::Size);
            Ok((self.width.get(), 24))
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Hide);
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Show);
            Ok(())
        }

        fn move_up(&mut self, rows: usize) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Up(rows));
            Ok(())
        }

        fn move_down(&mut self, rows: usize) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Down(rows));
            Ok(())
        }

        fn move_to_column(&mut self, column: usize) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Column(column));
            Ok(())
        }

        fn clear_line(&mut self) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Clear);
            Ok(())
        }

        fn write_text(&mut self, text: &str) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Write(text.to_string()));
            if self.fail_write {
                Err(io::Error::other("write failed"))
            } else {
                Ok(())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            self.operations.borrow_mut().push(Operation::Flush);
            Ok(())
        }
    }

    #[test]
    fn output_erases_then_writes_then_redraws_with_one_flush() {
        let (backend, operations, _) = FakeTerminal::new(10);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.write_output("answer\nnext").unwrap();

        let operations = operations.borrow();
        let output_index = operations
            .iter()
            .position(|operation| operation == &Operation::Write("answer\r\nnext".into()))
            .unwrap();
        let last_clear = operations
            .iter()
            .rposition(|operation| operation == &Operation::Clear)
            .unwrap();
        let divider_index = operations
            .iter()
            .position(|operation| operation == &Operation::Write("----------".into()))
            .unwrap();
        assert!(last_clear < output_index);
        assert!(output_index < divider_index);
        assert_eq!(
            operations
                .iter()
                .filter(|operation| operation == &&Operation::Flush)
                .count(),
            1
        );
    }

    #[test]
    fn resize_erases_using_old_layout_and_redraws_at_new_width() {
        let (backend, operations, width) = FakeTerminal::new(8);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();
        width.set(4);

        assert!(controller.refresh_size().unwrap());

        let operations = operations.borrow();
        let clear_index = operations
            .iter()
            .position(|operation| operation == &Operation::Clear)
            .unwrap();
        let divider_index = operations
            .iter()
            .position(|operation| operation == &Operation::Write("----".into()))
            .unwrap();
        assert!(clear_index < divider_index);
    }

    #[test]
    fn unchanged_size_does_not_redraw() {
        let (backend, operations, _) = FakeTerminal::new(8);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        assert!(!controller.refresh_size().unwrap());
        assert_eq!(*operations.borrow(), [Operation::Size]);
    }

    #[test]
    fn drop_erases_region_and_restores_terminal() {
        let (backend, operations, _) = FakeTerminal::new(8);
        let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        drop(controller);

        let operations = operations.borrow();
        assert!(operations.contains(&Operation::Clear));
        assert!(operations.ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush,]));
    }

    #[test]
    fn construction_error_restores_cursor_and_raw_mode() {
        let (mut backend, operations, _) = FakeTerminal::new(8);
        backend.fail_write = true;

        assert!(TerminalController::new(backend, InteractiveState::default()).is_err());

        let operations = operations.borrow();
        assert!(operations.contains(&Operation::Show));
        assert!(operations.contains(&Operation::Raw(false)));
    }
}
