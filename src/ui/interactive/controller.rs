use std::io::{self, Stdout, Write};

use anstyle::Style;
use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    queue,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use unicode_width::UnicodeWidthChar;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveToolBlock {
    pub name: String,
    pub args_summary: String,
    pub output: String,
    pub started: std::time::Instant,
}

pub struct TerminalController<B: TerminalBackend> {
    backend: B,
    state: InteractiveState,
    width: usize,
    rendered: Option<InteractiveLayout>,
    output_line: String,
    output_line_open: bool,
    spinner_frame: usize,
    active: bool,
    footer_style: Style,
    active_tool: Option<ActiveToolBlock>,
    active_tool_height: usize,
    theme: crate::ui::theme::Theme,
    transcript: Vec<super::TranscriptItem>,
}

impl TerminalController<CrosstermBackend> {
    pub fn stdout(state: InteractiveState) -> io::Result<Self> {
        Self::new(CrosstermBackend::stdout(), state)
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(mut backend: B, state: InteractiveState) -> io::Result<Self> {
        backend.set_raw_mode(true)?;
        let width = match backend.size() {
            Ok((width, _)) => usize::from(width),
            Err(error) => {
                let _ = backend.set_raw_mode(false);
                return Err(error);
            }
        };
        let mut controller = Self {
            backend,
            state,
            width,
            rendered: None,
            output_line: String::new(),
            output_line_open: false,
            spinner_frame: 0,
            active: true,
            footer_style: crate::ui::theme::Theme::default().dimmed,
            active_tool: None,
            active_tool_height: 0,
            theme: crate::ui::theme::Theme::default(),
            transcript: Vec::new(),
        };
        if let Err(error) = controller.redraw() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    pub fn start_tool(&mut self, name: String, args_summary: String) -> io::Result<()> {
        self.active_tool = Some(ActiveToolBlock {
            name,
            args_summary,
            output: String::new(),
            started: std::time::Instant::now(),
        });
        self.redraw()
    }

    pub fn append_tool_chunk(&mut self, chunk: &str) -> io::Result<()> {
        if let Some(tool) = &mut self.active_tool {
            tool.output.push_str(chunk);
        }
        self.redraw()
    }

    pub fn end_tool(&mut self) -> io::Result<()> {
        self.active_tool = None;
        self.redraw()
    }

    pub fn push_transcript_item(&mut self, item: super::TranscriptItem) -> io::Result<()> {
        let rendered = super::render_transcript_item(super::TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
        });
        self.transcript.push(item);
        if !rendered.is_empty() {
            self.write_output(&rendered)?;
        }
        Ok(())
    }

    pub fn full_redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.backend.write_text("\x1b[2J\x1b[H\x1b[3J")?;
        self.output_line.clear();
        self.output_line_open = false;

        let tools_expanded = self.state.tools_expanded();
        let rendered_items: Vec<String> = self
            .transcript
            .iter()
            .map(|item| {
                super::render_transcript_item(super::TranscriptRenderInput {
                    item,
                    theme: &self.theme,
                    width: self.width,
                    tools_expanded,
                })
            })
            .filter(|rendered| !rendered.is_empty())
            .map(|rendered| terminal_newlines(&rendered))
            .collect();

        for rendered in &rendered_items {
            self.backend.write_text(rendered)?;
            self.update_output_line(rendered);
            if self.output_line_open {
                self.backend.write_text("\r\n")?;
            }
        }

        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn state(&self) -> &InteractiveState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut InteractiveState {
        &mut self.state
    }

    pub fn terminal_width(&self) -> usize {
        self.width.max(1)
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.restore_output_cursor()?;
        let output = terminal_newlines(output);
        self.backend.write_text(&output)?;
        self.update_output_line(&output);
        if self.output_line_open {
            self.backend.write_text("\r\n")?;
        }
        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn refresh_size(&mut self) -> io::Result<bool> {
        let (width, _) = self.backend.size()?;
        let width = usize::from(width);
        if width == self.width {
            return Ok(false);
        }
        self.width = width;
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(true)
    }

    pub fn toggle_tools_expanded(&mut self) -> io::Result<bool> {
        let tools_expanded = self.state_mut().toggle_tools_expanded();
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(tools_expanded)
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.advance_spinner();
        self.redraw()
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.output_line.clear();
        self.output_line_open = false;
        self.backend.show_cursor()?;
        self.backend.set_raw_mode(false)?;
        self.backend.flush()?;
        self.active = false;
        Ok(())
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if self.active {
            return Ok(());
        }
        self.backend.set_raw_mode(true)?;
        self.active = true;
        self.redraw()
    }

    fn restore_output_cursor(&mut self) -> io::Result<()> {
        if !self.output_line_open {
            return Ok(());
        }
        let (column, at_wrap_boundary) = output_cursor(&self.output_line, self.width);
        if !at_wrap_boundary {
            self.backend.move_up(1)?;
        }
        self.backend.move_to_column(column)
    }

    fn update_output_line(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(newline) = output.rfind('\n') {
            self.output_line.clear();
            self.output_line.push_str(&output[newline + 1..]);
        } else {
            self.output_line.push_str(output);
        }
        self.output_line_open = !output.ends_with('\n');
    }

    fn current_layout(&self) -> InteractiveLayout {
        layout(LayoutInput {
            editor: self.state.editor(),
            modal: self.state.active_modal(),
            footer: self.state.footer(),
            queued_messages: self.state.queue_len(),
            terminal_width: self.width,
            spinner_frame: self.spinner_frame,
        })
    }

    fn write_live_region(&mut self, rendered: &InteractiveLayout) -> io::Result<()> {
        if !rendered.working_line.is_empty() {
            self.backend.write_text(&rendered.working_line)?;
            self.backend.write_text("\r\n")?;
        }
        if !rendered.top_divider.is_empty() {
            self.backend.write_text(&rendered.top_divider)?;
            self.backend.write_text("\r\n")?;
        }
        for line in &rendered.editor_lines {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
        }
        if !rendered.bottom_divider.is_empty() {
            self.backend.write_text(&rendered.bottom_divider)?;
            self.backend.write_text("\r\n")?;
        }
        let footer_style = self.footer_style;
        self.backend
            .write_text(&format!("{footer_style}{}{footer_style:#}", rendered.footer))?;

        self.backend.move_up(rendered.height() - 1 - rendered.cursor_row())?;
        self.backend.move_to_column(rendered.cursor.column)
    }

    fn erase_live_region(&mut self) -> io::Result<()> {
        let Some(rendered) = self.rendered.as_ref() else {
            return Ok(());
        };
        let height = rendered.height();
        let cursor_row = rendered.cursor_row();
        self.backend.move_down(height - 1 - cursor_row)?;
        self.backend.move_to_column(0)?;
        for row in (0..height).rev() {
            self.backend.clear_line()?;
            if row > 0 {
                self.backend.move_up(1)?;
            }
        }
        self.backend.move_to_column(0)?;
        self.rendered = None;
        Ok(())
    }

    fn erase_active_tool(&mut self) -> io::Result<()> {
        if self.active_tool_height == 0 {
            return Ok(());
        }
        self.backend.move_up(self.active_tool_height)?;
        self.backend.move_to_column(0)?;
        for row in 0..self.active_tool_height {
            self.backend.clear_line()?;
            if row + 1 < self.active_tool_height {
                self.backend.move_down(1)?;
            }
        }
        self.backend.move_up(self.active_tool_height.saturating_sub(1))?;
        self.backend.move_to_column(0)?;
        self.active_tool_height = 0;
        Ok(())
    }

    fn draw_active_tool(&mut self) -> io::Result<()> {
        let Some(tool) = &self.active_tool else {
            self.active_tool_height = 0;
            return Ok(());
        };
        let formatted = super::layout::format_active_tool_block(super::ActiveToolDisplayInput {
            tool_name: &tool.name,
            args_summary: &tool.args_summary,
            output: &tool.output,
            started: tool.started,
            theme: &self.theme,
            width: self.width,
            expanded: self.state.tools_expanded(),
        });
        let formatted = terminal_newlines(&formatted);
        let mut count = 0;
        for line in formatted.split("\r\n") {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
            count += 1;
        }
        self.active_tool_height = count;
        Ok(())
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.erase_live_region();
        let _ = self.erase_active_tool();
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

fn output_cursor(value: &str, terminal_width: usize) -> (usize, bool) {
    let terminal_width = terminal_width.max(1);
    let mut column = 0;
    let mut at_wrap_boundary = false;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.next_if_eq(&'[').is_some() {
                for sequence_character in characters.by_ref() {
                    if ('@'..='~').contains(&sequence_character) {
                        break;
                    }
                }
            }
            continue;
        }
        if character == '\r' {
            column = 0;
            at_wrap_boundary = false;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > terminal_width {
            column = 0;
        }
        column += character_width;
        at_wrap_boundary = column == terminal_width;
        if at_wrap_boundary {
            column = 0;
        }
    }
    (column, at_wrap_boundary)
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

    use super::{TerminalBackend, TerminalController, output_cursor};
    use crate::ui::interactive::{Activity, InteractiveState, OutputEvent, PendingUiBatch, UiEvent};

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

    type SharedOperations = Rc<RefCell<Vec<Operation>>>;
    type SharedWidth = Rc<Cell<u16>>;

    struct FakeTerminal {
        operations: SharedOperations,
        width: SharedWidth,
        fail_write: bool,
    }

    impl FakeTerminal {
        fn new(width: u16) -> (Self, SharedOperations, SharedWidth) {
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
    fn output_cursor_tracks_wrap_boundaries_styles_and_wide_text() {
        assert_eq!(output_cursor("123456789", 10), (9, false));
        assert_eq!(output_cursor("1234567890", 10), (0, true));
        assert_eq!(output_cursor("123456789界", 10), (2, false));
        assert_eq!(output_cursor("\u{1b}[2mwide\u{1b}[0m", 10), (4, false));
    }

    #[test]
    fn construction_positions_and_shows_the_editor_cursor() {
        let (backend, operations, _) = FakeTerminal::new(10);

        let _controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        let operations = operations.borrow();
        let show_index = operations
            .iter()
            .rposition(|operation| operation == &Operation::Show)
            .unwrap();
        let flush_index = operations
            .iter()
            .rposition(|operation| operation == &Operation::Flush)
            .unwrap();
        assert!(operations[..show_index].contains(&Operation::Hide));
        assert!(show_index < flush_index);
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
            .position(|operation| operation == &Operation::Write("──────────".into()))
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
    fn many_stream_fragments_are_written_with_one_controller_flush() {
        let (backend, operations, _) = FakeTerminal::new(40);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();
        let mut pending = PendingUiBatch::new(16 * 1024);
        for _ in 0..1_000 {
            pending.push(UiEvent::Output(OutputEvent::Text("token".into())));
        }

        controller.write_output(&pending.drain().text).unwrap();

        let operations = operations.borrow();
        assert_eq!(
            operations
                .iter()
                .filter(|operation| operation == &&Operation::Write("token".repeat(1_000)))
                .count(),
            1
        );
        assert_eq!(
            operations
                .iter()
                .filter(|operation| operation == &&Operation::Flush)
                .count(),
            1
        );
    }

    #[test]
    fn streamed_output_resumes_at_the_previous_line_end() {
        let (backend, operations, _) = FakeTerminal::new(10);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.write_output("streamed ").unwrap();
        operations.borrow_mut().clear();
        controller.write_output("response").unwrap();

        let operations = operations.borrow();
        let move_index = operations
            .iter()
            .position(|operation| operation == &Operation::Up(1))
            .unwrap();
        let column_index = operations
            .iter()
            .position(|operation| operation == &Operation::Column(9))
            .unwrap();
        let output_index = operations
            .iter()
            .position(|operation| operation == &Operation::Write("response".into()))
            .unwrap();
        assert!(move_index < column_index);
        assert!(column_index < output_index);
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
            .position(|operation| operation == &Operation::Write("────".into()))
            .unwrap();
        assert!(clear_index < divider_index);
    }

    #[test]
    fn resize_rerenders_active_tool_block_at_new_width() {
        let (backend, operations, width) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller.start_tool("bash".into(), "cargo test".into()).unwrap();
        operations.borrow_mut().clear();
        width.set(30);

        assert!(controller.refresh_size().unwrap());

        let ops = operations.borrow();
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("bash") && text.contains("cargo test")))
        );
    }

    #[test]
    fn tick_redraws_the_live_region() {
        let (backend, operations, _) = FakeTerminal::new(8);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.tick().unwrap();

        let operations = operations.borrow();
        assert!(operations.contains(&Operation::Clear));
        assert!(operations.contains(&Operation::Write("────────".into())));
        assert!(operations.ends_with(&[Operation::Show, Operation::Flush]));
    }

    #[test]
    fn busy_working_line_renders_above_the_editor() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut state = InteractiveState::default();
        state.footer_mut().activity = Activity::Thinking;
        let mut controller = TerminalController::new(backend, state).unwrap();
        operations.borrow_mut().clear();

        controller.tick().unwrap();

        let ops = operations.borrow();
        let working_index = ops.iter().position(|op| {
            matches!(
                op,
                Operation::Write(text) if text.contains("Working...")
            )
        });
        let divider_index = ops.iter().position(|op| op == &Operation::Write("\u{2500}".repeat(60)));

        assert!(working_index.is_some());
        assert!(working_index.unwrap() < divider_index.unwrap());
    }

    #[test]
    fn busy_working_line_disappears_when_idle() {
        let (backend, operations, _) = FakeTerminal::new(20);
        let mut state = InteractiveState::default();
        state.footer_mut().activity = Activity::Working;
        let mut controller = TerminalController::new(backend, state).unwrap();
        controller.state_mut().footer_mut().activity = Activity::Idle;
        operations.borrow_mut().clear();

        controller.tick().unwrap();

        let ops = operations.borrow();
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("Working...")))
        );
    }

    #[test]
    fn footer_carries_no_spinner_or_activity_label_when_busy() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut state = InteractiveState::default();
        state.footer_mut().activity = Activity::Working;
        state.footer_mut().model = "model".into();
        let mut controller = TerminalController::new(backend, state).unwrap();
        operations.borrow_mut().clear();

        controller.tick().unwrap();

        let ops = operations.borrow();
        assert!(ops.contains(&Operation::Write("\u{1b}[2mmodel\u{1b}[0m".into())));
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("working")))
        );
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("thinking")))
        );
    }

    #[test]
    fn idle_footer_is_rendered_dimmed() {
        let (backend, operations, _) = FakeTerminal::new(20);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.tick().unwrap();

        assert!(
            operations
                .borrow()
                .contains(&Operation::Write("\u{1b}[2midle\u{1b}[0m".into()))
        );
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
    fn suspend_and_resume_restore_terminal_modes_around_legacy_prompts() {
        let (backend, operations, _) = FakeTerminal::new(8);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.suspend().unwrap();
        assert!(
            operations
                .borrow()
                .ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush,])
        );
        operations.borrow_mut().clear();
        controller.resume().unwrap();
        assert_eq!(operations.borrow().first(), Some(&Operation::Raw(true)));
        assert!(operations.borrow().ends_with(&[Operation::Show, Operation::Flush]));
    }

    #[test]
    fn active_tool_block_updates_inplace_and_cleans_up_on_end() {
        let (backend, operations, _) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        operations.borrow_mut().clear();

        controller.start_tool("bash".into(), "cargo test".into()).unwrap();
        let ops = operations.borrow();
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("bash") && text.contains("cargo test")))
        );
        drop(ops);

        operations.borrow_mut().clear();
        controller.append_tool_chunk("line 1\nline 2\n").unwrap();
        let ops = operations.borrow();
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("line 1")))
        );
        drop(ops);

        operations.borrow_mut().clear();
        controller.end_tool().unwrap();
        let ops = operations.borrow();
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("cargo test")))
        );
    }

    #[test]
    fn full_redraw_rerenders_all_transcript_items_on_resize() {
        let (backend, operations, width) = FakeTerminal::new(60);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(crate::ui::interactive::TranscriptItem::UserMessage(
                "hello world message".into(),
            ))
            .unwrap();
        operations.borrow_mut().clear();

        width.set(40);
        assert!(controller.refresh_size().unwrap());

        let ops = operations.borrow();
        assert!(ops.contains(&Operation::Write("\x1b[2J\x1b[H\x1b[3J".into())));
        assert!(
            ops.iter()
                .any(|op| matches!(op, Operation::Write(text) if text.contains("hello world message")))
        );
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
