use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use crate::ui::interactive::InteractiveState;
use crate::ui::interactive::controller::{TerminalBackend, TerminalController};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
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

pub type SharedOperations = Rc<RefCell<Vec<Operation>>>;
pub type SharedWidth = Rc<Cell<u16>>;
pub type SharedHeight = Rc<Cell<u16>>;

pub struct FakeTerminal {
    pub operations: SharedOperations,
    pub width: SharedWidth,
    pub height: SharedHeight,
    pub fail_write: bool,
}

impl FakeTerminal {
    pub fn new(width: u16) -> (Self, SharedOperations, SharedWidth) {
        let (term, ops, w, _) = Self::with_size(width, 24);
        (term, ops, w)
    }

    pub fn with_size(width: u16, height: u16) -> (Self, SharedOperations, SharedWidth, SharedHeight) {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let width = Rc::new(Cell::new(width));
        let height = Rc::new(Cell::new(height));
        (
            Self {
                operations: Rc::clone(&operations),
                width: Rc::clone(&width),
                height: Rc::clone(&height),
                fail_write: false,
            },
            operations,
            width,
            height,
        )
    }
}

pub fn controller(width: u16) -> (TerminalController<FakeTerminal>, SharedOperations, SharedWidth) {
    let (backend, operations, width) = FakeTerminal::new(width);
    (controller_for(backend), operations, width)
}

pub fn controller_with_size(
    width: u16,
    height: u16,
) -> (
    TerminalController<FakeTerminal>,
    SharedOperations,
    SharedWidth,
    SharedHeight,
) {
    let (backend, operations, width, height) = FakeTerminal::with_size(width, height);
    (controller_for(backend), operations, width, height)
}

pub fn write_count(ops: &[Operation], needle: &str) -> usize {
    ops.iter()
        .filter(|op| matches!(op, Operation::Write(text) if text.contains(needle)))
        .count()
}

pub fn wrote(ops: &[Operation], needle: &str) -> bool {
    write_count(ops, needle) > 0
}

fn controller_for(backend: FakeTerminal) -> TerminalController<FakeTerminal> {
    TerminalController::new(backend, InteractiveState::default()).unwrap()
}

impl TerminalBackend for FakeTerminal {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Raw(enabled));
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        self.operations.borrow_mut().push(Operation::Size);
        Ok((self.width.get(), self.height.get()))
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
