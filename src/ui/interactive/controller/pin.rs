//! Keeps the live region on the last rows of the screen without taking the screen over: output
//! still prints inline and scrolls, but while it is short the region is drawn under blank rows so
//! its last row is the screen's last row, and a region that shrinks over scrolled output brings
//! the rows it uncovered back.

use std::collections::VecDeque;

use super::sgr::SgrStyle;
use crate::ui::terminal_stream::{Cursor, Token, tokens};

/// Printed rows kept for repainting, which bounds the memory of a long session.
const MAX_KEPT_ROWS: usize = 4000;

/// Every row printed above the live region since the screen was last cleared, as a terminal
/// lays them out, each repeatable on its own.
#[derive(Debug)]
struct PrintedRows {
    cursor: Cursor,
    finished: VecDeque<String>,
    finished_total: usize,
    row: String,
    row_style: SgrStyle,
    style: SgrStyle,
    overwrite: bool,
}

impl PrintedRows {
    fn new(width: usize) -> Self {
        Self {
            cursor: Cursor::new(width),
            finished: VecDeque::new(),
            finished_total: 0,
            row: String::new(),
            row_style: SgrStyle::default(),
            style: SgrStyle::default(),
            overwrite: false,
        }
    }

    fn is_open(&self) -> bool {
        !self.cursor.is_at_line_start()
    }

    /// Rows the printed output covers, counting a line still being written.
    fn occupied(&self) -> usize {
        self.finished_total + usize::from(self.is_open())
    }

    /// Rows still kept, which is fewer than `occupied` once the oldest were dropped.
    fn available(&self) -> usize {
        self.finished.len() + usize::from(self.is_open())
    }

    fn tail(&self, count: usize) -> Vec<String> {
        let open = self
            .is_open()
            .then(|| format!("{}{}", self.row_style.as_str(), self.row));
        let skip = self.available().saturating_sub(count);
        self.finished.iter().cloned().chain(open).skip(skip).collect()
    }

    fn feed(&mut self, text: &str) {
        for token in tokens(text) {
            match token {
                Token::Escape { raw, sgr } => {
                    self.row.push_str(raw);
                    if let Some(parameters) = sgr {
                        self.style.apply(parameters);
                    }
                }
                Token::Char('\r') => {
                    self.cursor.return_to_line_start();
                    self.overwrite = true;
                }
                Token::Char('\n') => self.finish_row(),
                Token::Char(character) => self.print(character),
            }
        }
    }

    fn print(&mut self, character: char) {
        if std::mem::take(&mut self.overwrite) {
            self.row.clear();
        }
        if self.cursor.wraps_before(character) {
            self.finish_row();
        }
        self.row.push(character);
        self.cursor.advance(character);
    }

    fn finish_row(&mut self) {
        if self.finished.len() == MAX_KEPT_ROWS {
            self.finished.pop_front();
        }
        let row = format!("{}{}", self.row_style.as_str(), self.row);
        self.finished.push_back(row);
        self.finished_total += 1;
        self.row.clear();
        self.row_style = self.style.clone();
        self.cursor.return_to_line_start();
        self.overwrite = false;
    }
}

/// How the live region is drawn next.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Fit {
    /// Under this many blank rows.
    Pad(usize),
    /// After repainting the printed rows, which the region would otherwise leave a gap above.
    Repaint,
}

#[derive(Debug)]
pub(super) struct BottomPin {
    rows: PrintedRows,
    /// Printed rows on the screen above the live region: the row the region (its pad included)
    /// starts on.
    top: usize,
    pad: usize,
}

impl BottomPin {
    pub(super) fn new(width: usize) -> Self {
        Self {
            rows: PrintedRows::new(width),
            top: 0,
            pad: 0,
        }
    }

    pub(super) fn pad(&self) -> usize {
        self.pad
    }

    /// Starts over on a cleared screen `width` columns wide.
    pub(super) fn clear(&mut self, width: usize) {
        *self = Self::new(width);
    }

    /// Records text printed above the region.
    pub(super) fn print(&mut self, text: &str) {
        let before = self.rows.occupied();
        self.rows.feed(text);
        self.top += self.rows.occupied().saturating_sub(before);
    }

    /// The pad for a region `region` rows tall on a screen `screen` rows tall: enough blank rows to
    /// end on the last row, none when the printed rows already fill the screen. A region taller
    /// than the free rows scrolls the printed rows up.
    pub(super) fn settle(&mut self, region: usize, screen: usize) -> usize {
        match screen.checked_sub(self.top + region) {
            Some(free) => self.pad = free,
            None => {
                self.top = screen.saturating_sub(region);
                self.pad = 0;
            }
        }
        self.pad
    }

    /// Like `settle`, but asks for a repaint when a shorter region would leave blank rows above it
    /// while printed rows sit scrolled off the screen.
    pub(super) fn fit(&mut self, region: usize, screen: usize) -> Fit {
        let gap = screen.checked_sub(self.top + region).is_some_and(|free| free > 0);
        if gap && self.rows.available() > self.top {
            return Fit::Repaint;
        }
        Fit::Pad(self.settle(region, screen))
    }

    /// The printed rows to show above a region `region` rows tall, newest last.
    pub(super) fn repaint_rows(&mut self, region: usize, screen: usize) -> Vec<String> {
        let rows = self.rows.tail(screen.saturating_sub(region));
        self.top = rows.len();
        self.settle(region, screen);
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(width: usize, text: &str) -> PrintedRows {
        let mut rows = PrintedRows::new(width);
        rows.feed(text);
        rows
    }

    #[test]
    fn rows_count_lines_wraps_and_the_line_being_written() {
        let rows = printed(10, "one\r\ntwo\r\n0123456789abc");
        assert_eq!((rows.occupied(), rows.available()), (4, 4));
        assert_eq!(rows.tail(4), ["one", "two", "0123456789", "abc"]);
    }

    #[test]
    fn a_line_filling_its_row_wraps_only_when_more_is_printed() {
        let full = printed(5, "abcde");
        assert_eq!(full.occupied(), 1);
        let closed = printed(5, "abcde\r\nnext\r\n");
        assert_eq!(closed.tail(9), ["abcde", "next"]);
        let wrapped = printed(5, "abcdef");
        assert_eq!(wrapped.tail(9), ["abcde", "f"]);
    }

    #[test]
    fn blank_lines_are_rows_and_an_empty_stream_has_none() {
        assert_eq!(PrintedRows::new(10).occupied(), 0);
        assert_eq!(printed(10, "a\r\n\r\n\r\nb\r\n").tail(9), ["a", "", "", "b"]);
    }

    #[test]
    fn wide_characters_wrap_by_their_width() {
        let rows = printed(4, "ab\u{4e16}\u{754c}");
        assert_eq!(rows.tail(9), ["ab\u{4e16}", "\u{754c}"]);
    }

    #[test]
    fn a_row_keeps_the_style_in_force_where_it_starts() {
        let rows = printed(10, "\u{1b}[1mbold\r\nstill bold\u{1b}[0m\r\nplain\r\n");
        assert_eq!(rows.tail(9), ["\u{1b}[1mbold", "\u{1b}[1mstill bold\u{1b}[0m", "plain"]);
    }

    #[test]
    fn a_bare_carriage_return_overwrites_the_row() {
        assert_eq!(printed(10, "10%\r20%\r\n").tail(9), ["20%"]);
    }

    #[test]
    fn only_the_newest_rows_are_kept_but_all_are_counted() {
        let mut rows = PrintedRows::new(10);
        for n in 0..MAX_KEPT_ROWS + 5 {
            rows.feed(&format!("{n}\r\n"));
        }
        assert_eq!(rows.occupied(), MAX_KEPT_ROWS + 5);
        assert_eq!(rows.available(), MAX_KEPT_ROWS);
        assert_eq!(rows.tail(1), [(MAX_KEPT_ROWS + 4).to_string()]);
    }

    #[test]
    fn a_short_screen_of_output_pads_the_region_to_the_last_row() {
        let mut pin = BottomPin::new(40);
        assert_eq!(pin.fit(4, 20), Fit::Pad(16));
        pin.print("a\r\nb\r\n");
        assert_eq!(pin.fit(4, 20), Fit::Pad(14));
        assert_eq!(pin.fit(9, 20), Fit::Pad(9));
        assert_eq!(pin.fit(4, 20), Fit::Pad(14));
    }

    #[test]
    fn output_that_fills_the_screen_leaves_no_pad_and_scrolls() {
        let mut pin = BottomPin::new(40);
        for n in 0..30 {
            pin.print(&format!("line {n}\r\n"));
        }
        assert_eq!(pin.fit(4, 20), Fit::Pad(0));
        assert_eq!(pin.fit(10, 20), Fit::Pad(0));
    }

    #[test]
    fn a_region_shrinking_over_scrolled_output_asks_for_a_repaint_that_brings_rows_back() {
        let mut pin = BottomPin::new(40);
        for n in 0..30 {
            pin.print(&format!("line {n}\r\n"));
        }
        assert_eq!(pin.fit(10, 20), Fit::Pad(0));
        assert_eq!(pin.fit(4, 20), Fit::Repaint);
        let rows = pin.repaint_rows(4, 20);
        assert_eq!(
            (rows.len(), rows[0].as_str(), rows[15].as_str()),
            (16, "line 14", "line 29")
        );
        assert_eq!(pin.pad(), 0);
        assert_eq!(pin.fit(4, 20), Fit::Pad(0));
    }

    #[test]
    fn a_shrinking_region_pads_instead_when_every_row_is_still_on_screen() {
        let mut pin = BottomPin::new(40);
        pin.print("only\r\nthree\r\nrows\r\n");
        assert_eq!(pin.fit(12, 20), Fit::Pad(5));
        assert_eq!(pin.fit(5, 20), Fit::Pad(12));
    }

    #[test]
    fn clearing_the_screen_forgets_the_printed_rows() {
        let mut pin = BottomPin::new(40);
        pin.print(&"x\r\n".repeat(30));
        assert_eq!(pin.fit(10, 20), Fit::Pad(0));
        pin.clear(40);
        pin.print("a\r\n");
        assert_eq!(pin.fit(4, 20), Fit::Pad(15));
    }
}
