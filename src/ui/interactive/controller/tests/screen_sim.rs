//! Screen-simulating backend for repaint regression tests: the fake terminal
//! maintains a physical character grid (deferred wrap, scroll, cursor moves)
//! so tests can assert that the visible screen matches the controller layout.

use crate::ui::interactive::controller::{TerminalBackend, TerminalController};
use std::io;

pub struct ScreenBackend {
    pub width: usize,
    pub height: usize,
    pub grid: Vec<Vec<char>>,
    pub row: usize,
    pub col: usize,
    pending_wrap: bool,
}

impl ScreenBackend {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            grid: vec![vec![' '; width]; height],
            row: 0,
            col: 0,
            pending_wrap: false,
        }
    }

    fn scroll_up(&mut self) {
        self.grid.remove(0);
        self.grid.push(vec![' '; self.width]);
    }

    fn newline(&mut self) {
        self.pending_wrap = false;
        self.col = 0;
        if self.row + 1 >= self.height {
            self.scroll_up();
        } else {
            self.row += 1;
        }
    }

    fn put(&mut self, c: char, w: usize) {
        if self.pending_wrap {
            self.newline();
        }
        self.grid[self.row][self.col] = c;
        self.col += w;
        if self.col >= self.width {
            self.pending_wrap = true;
        }
    }

    fn text(&self) -> Vec<String> {
        self.grid
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    fn run_text(&mut self, text: &str) {
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\x1b' => i = self.run_escape(&chars, i + 1),
                '\r' => {
                    self.col = 0;
                    self.pending_wrap = false;
                    i += 1;
                }
                '\n' => {
                    self.newline();
                    i += 1;
                }
                _ => {
                    let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
                    if w > 0 {
                        self.put(chars[i], w);
                    }
                    i += 1;
                }
            }
        }
    }

    fn run_escape(&mut self, chars: &[char], start: usize) -> usize {
        match chars.get(start) {
            Some('[') => {
                let mut i = start + 1;
                let mut params = String::new();
                while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                    params.push(chars[i]);
                    i += 1;
                }
                if let Some(&final_byte) = chars.get(i) {
                    self.run_csi(&params, final_byte);
                    i + 1
                } else {
                    i
                }
            }
            Some(']') => {
                let mut i = start + 1;
                while i < chars.len() && chars[i] != '\x07' {
                    if chars[i] == '\x1b' && chars.get(i + 1) == Some(&'\\') {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                i + 1
            }
            _ => start + 1,
        }
    }

    fn run_csi(&mut self, params: &str, final_byte: char) {
        let count = |p: &str| -> usize {
            let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<usize>().unwrap_or(1)
        };
        match final_byte {
            'A' => {
                self.row = self.row.saturating_sub(count(params));
                self.pending_wrap = false;
            }
            'B' => {
                self.row = (self.row + count(params)).min(self.height - 1);
                self.pending_wrap = false;
            }
            'G' => {
                self.col = (count(params).max(1) - 1).min(self.width - 1);
                self.pending_wrap = false;
            }
            'H' => {
                let mut parts = params.split(';');
                let r = parts.next().and_then(|p| p.parse::<usize>().ok()).unwrap_or(1);
                let c = parts.next().and_then(|p| p.parse::<usize>().ok()).unwrap_or(1);
                self.row = (r.max(1) - 1).min(self.height - 1);
                self.col = (c.max(1) - 1).min(self.width - 1);
                self.pending_wrap = false;
            }
            'J' => {
                if matches!(params.trim(), "2" | "3") {
                    for row in self.grid.iter_mut() {
                        for cell in row.iter_mut() {
                            *cell = ' ';
                        }
                    }
                    self.pending_wrap = false;
                }
            }
            'K' => match params.trim() {
                "0" => {
                    for cell in self.grid[self.row][self.col..].iter_mut() {
                        *cell = ' ';
                    }
                }
                "1" => {
                    for cell in self.grid[self.row][..=self.col].iter_mut() {
                        *cell = ' ';
                    }
                }
                _ => {
                    for cell in self.grid[self.row].iter_mut() {
                        *cell = ' ';
                    }
                }
            },
            _ => {}
        }
    }
}

impl TerminalBackend for ScreenBackend {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((self.width as u16, self.height as u16))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        self.run_csi(&rows.to_string(), 'A');
        Ok(())
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        self.run_csi(&rows.to_string(), 'B');
        Ok(())
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        self.run_csi(&(column + 1).to_string(), 'G');
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.run_csi("2", 'K');
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.run_text(text);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' {
            i = match chars.get(i + 1) {
                Some('[') => {
                    let mut j = i + 2;
                    while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    j + 1
                }
                Some(']') => {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] != '\x07' {
                        if chars[j] == '\x1b' && chars.get(j + 1) == Some(&'\\') {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    j + 1
                }
                _ => i + 2,
            };
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The live region must occupy exactly `rendered.height()` rows ending at the
/// last non-empty screen row, matching the cached layout (ANSI stripped). A
/// wrapped region row shifts the physical rows and desyncs this alignment.
fn assert_screen_region_aligned(controller: &TerminalController<ScreenBackend>) {
    let rendered = controller.rendered().expect("rendered layout").clone();
    let height = rendered.height();
    if height == 0 {
        return;
    }
    let screen = controller.backend.text();
    let last_nonempty = screen
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .expect("nonempty screen");
    let region_top = last_nonempty + 1 - height;
    let expected: Vec<String> = rendered
        .lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let actual: Vec<String> = screen[region_top..=last_nonempty]
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect();
    assert_eq!(actual, expected, "screen rows diverged from cached layout");
}

mod regressions {
    use super::{ScreenBackend, assert_screen_region_aligned};
    use crate::ui::interactive::controller::TerminalController;
    use crate::ui::interactive::{InteractiveState, ModalOption, ModalState, TranscriptItem};

    fn model_options(count: usize) -> Vec<ModalOption> {
        (0..count)
            .map(|i| {
                ModalOption::new(
                    format!("model-{i}-20260101"),
                    Some(format!("prov\t\t\t{} ctx", 100 + i)),
                )
            })
            .collect()
    }

    fn controller_with_transcript((width, height): (usize, usize)) -> TerminalController<ScreenBackend> {
        let mut controller =
            TerminalController::new(ScreenBackend::new(width, height), InteractiveState::default()).unwrap();
        controller
            .push_transcript_item(TranscriptItem::UserMessage("review the recent changes please".into()))
            .unwrap();
        let filler: Vec<String> = (0..20)
            .map(|i| format!("assistant output line {i} of the answer continues here"))
            .collect();
        controller.write_output(&format!("{}\n", filler.join("\n"))).unwrap();
        controller
    }

    fn open_selector(controller: &mut TerminalController<ScreenBackend>, count: usize) {
        let mut modal = ModalState::new("Select Model", "", model_options(count)).with_search(true);
        modal.selected = 1;
        controller.state_mut().push_modal(modal);
        controller.redraw().unwrap();
        assert_screen_region_aligned(controller);
    }

    fn arrow_down(controller: &mut TerminalController<ScreenBackend>, steps: usize) {
        for _ in 0..steps {
            controller.state_mut().select_next_modal_option();
            controller.redraw().unwrap();
            assert_screen_region_aligned(controller);
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
        assert_screen_region_aligned(&controller);

        arrow_down(&mut controller, 6);
    }
}
