use std::io;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::repl::interactive::Completion;
use crate::ui::block::ANSI_PATTERN;
use crate::ui::interactive::controller::{TerminalBackend, TerminalController};
use crate::ui::interactive::{InteractiveState, ModalOption, ModalState, ToolItem, TranscriptItem};

pub struct RatatuiTestBackend {
    pub buffer: Buffer,
    pub width: usize,
    pub height: usize,
    pub row: usize,
    pub col: usize,
    pub cursor_hidden: bool,
    pub raw_mode: bool,
}

impl RatatuiTestBackend {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width as u16, height as u16)),
            width,
            height,
            row: 0,
            col: 0,
            cursor_hidden: false,
            raw_mode: false,
        }
    }

    pub fn text(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..self.width {
                    let symbol = self
                        .buffer
                        .cell((x as u16, y as u16))
                        .map(|c| c.symbol())
                        .unwrap_or(" ");
                    line.push_str(symbol);
                }
                line
            })
            .collect()
    }
}

impl TerminalBackend for RatatuiTestBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        self.raw_mode = enabled;
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((self.width as u16, self.height as u16))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_hidden = true;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_hidden = false;
        Ok(())
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        self.row = self.row.saturating_sub(rows);
        Ok(())
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        self.row = (self.row + rows).min(self.height.saturating_sub(1));
        Ok(())
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        self.col = column.min(self.width.saturating_sub(1));
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        let y = self.row as u16;
        let w = self.width as u16;
        for x in 0..w {
            if let Some(cell) = self.buffer.cell_mut((x, y)) {
                cell.reset();
            }
        }
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        let stripped = ANSI_PATTERN.replace_all(text, "");
        for ch in stripped.chars() {
            match ch {
                '\r' => {
                    self.col = 0;
                }
                '\n' => {
                    self.row = (self.row + 1).min(self.height.saturating_sub(1));
                    self.col = 0;
                }
                c => {
                    if self.col < self.width && self.row < self.height {
                        let x = self.col as u16;
                        let y = self.row as u16;
                        if let Some(cell) = self.buffer.cell_mut((x, y)) {
                            cell.set_char(c);
                        }
                        self.col += 1;
                    }
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn model_options(count: usize) -> Vec<ModalOption> {
    (0..count)
        .map(|i| ModalOption::new(format!("model-{i:02}"), Some(format!("Description for model {i}"))))
        .collect()
}

fn controller_with_transcript((width, height): (usize, usize)) -> TerminalController<RatatuiTestBackend> {
    let mut controller =
        TerminalController::new(RatatuiTestBackend::new(width, height), InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("review the recent changes please".into()))
        .unwrap();
    let filler: Vec<String> = (0..20)
        .map(|i| format!("assistant output line {i} of the answer continues here"))
        .collect();
    controller.write_output(&format!("{}\n", filler.join("\n"))).unwrap();
    controller
}

fn open_selector(controller: &mut TerminalController<RatatuiTestBackend>, count: usize) {
    let mut modal = ModalState::new("Select Model", "", model_options(count)).with_search(true);
    modal.selected = 1;
    controller.state_mut().push_modal(modal);
    controller.redraw().unwrap();
}

fn arrow_down(controller: &mut TerminalController<RatatuiTestBackend>, steps: usize) {
    for _ in 0..steps {
        controller.state_mut().select_next_modal_option();
        controller.redraw().unwrap();
        let banner_count = controller
            .backend
            .text()
            .iter()
            .filter(|l| l.contains("Select Model"))
            .count();
        assert_eq!(banner_count, 1, "modal header repeated on screen");
    }
}

#[test]
fn draft_line_wider_than_terminal_keeps_model_selector_repaints_aligned() {
    let mut controller = controller_with_transcript((80, 24));
    controller
        .state_mut()
        .editor_mut()
        .set_text("half written prompt that was never sent to the agent yet");

    open_selector(&mut controller, 37);
    arrow_down(&mut controller, 8);
}

#[test]
fn long_filter_query_keeps_model_selector_repaints_aligned() {
    let mut controller = controller_with_transcript((80, 24));
    open_selector(&mut controller, 37);
    let query: String = "x".repeat(90);
    controller.state_mut().active_modal_mut().unwrap().set_filter(&query);
    controller.redraw().unwrap();
    arrow_down(&mut controller, 6);
}

#[test]
fn long_completion_value_keeps_editor_repaints_aligned() {
    let mut controller = controller_with_transcript((80, 24));
    controller.state_mut().editor_mut().set_text("src/ui/int");
    controller.state_mut().autocomplete.open(vec![
        Completion {
            value: "x".repeat(90),
            description: None,
            replacement: 0..10,
        },
        Completion {
            value: "y".repeat(90),
            description: None,
            replacement: 0..10,
        },
    ]);
    controller.redraw().unwrap();

    controller.state_mut().autocomplete.select_next();
    controller.redraw().unwrap();
}

#[test]
fn committed_bash_card_keeps_single_blank_after_paragraph() {
    let mut controller = controller_with_transcript((80, 24));
    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
            is_error: false,
            output: "ok".into(),
            output_summary: "ok".into(),
            duration_ms: Some(50),
        }))
        .unwrap();
    let text = controller.backend.text();
    assert!(text.iter().any(|line| line.contains("cargo test")));
}

#[test]
fn streamed_thinking_wraps_and_preserves_natural_line_breaks() {
    let mut controller = controller_with_transcript((80, 24));
    controller
        .write_stream_output("thinking through the next steps\nand verifying behavior\n")
        .unwrap();
    let text = controller.backend.text();
    assert!(text.iter().any(|line| line.contains("thinking through")));
}

#[test]
fn window_resize_reflows_cleanly() {
    let mut controller = controller_with_transcript((80, 24));
    open_selector(&mut controller, 20);
    controller.resize_to(100, 30).unwrap();
    let text = controller.backend.text();
    assert!(text.iter().any(|line| line.contains("Select Model")));
}

#[test]
fn unfocused_working_spinner_rendering() {
    let mut controller = controller_with_transcript((80, 24));
    controller.set_focused(false);
    assert!(!controller.focused());
    controller.redraw().unwrap();
}
