//! 2D character canvas with junction-merging and box drawing.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const DIR_N: u8 = 1 << 0;
const DIR_S: u8 = 1 << 1;
const DIR_E: u8 = 1 << 2;
const DIR_W: u8 = 1 << 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowDir {
    Down,
    Up,
    Right,
    Left,
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    chars: Vec<Vec<char>>,
    masks: Vec<Vec<u8>>,
    locked: Vec<Vec<bool>>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            chars: vec![vec![' '; width]; height],
            masks: vec![vec![0; width]; height],
            locked: vec![vec![false; width]; height],
        }
    }

    pub fn ensure_size(&mut self, required_w: usize, required_h: usize) {
        if required_h > self.height {
            for _ in self.height..required_h {
                self.chars.push(vec![' '; self.width]);
                self.masks.push(vec![0; self.width]);
                self.locked.push(vec![false; self.width]);
            }
            self.height = required_h;
        }
        if required_w > self.width {
            for row in &mut self.chars {
                row.resize(required_w, ' ');
            }
            for row in &mut self.masks {
                row.resize(required_w, 0);
            }
            for row in &mut self.locked {
                row.resize(required_w, false);
            }
            self.width = required_w;
        }
    }

    pub fn put_char(&mut self, x: usize, y: usize, ch: char, locked: bool) {
        if x >= self.width || y >= self.height {
            self.ensure_size(x + 1, y + 1);
        }
        self.chars[y][x] = ch;
        self.locked[y][x] = locked;
    }

    pub fn add_mask(&mut self, x: usize, y: usize, mask: u8) {
        if x >= self.width || y >= self.height {
            self.ensure_size(x + 1, y + 1);
        }
        if self.locked[y][x] {
            return;
        }
        self.masks[y][x] |= mask;
        self.chars[y][x] = mask_to_glyph(self.masks[y][x]);
    }

    pub fn draw_h_line(&mut self, y: usize, x1: usize, x2: usize) {
        let (start, end) = (x1.min(x2), x1.max(x2));
        for x in start..=end {
            let mut mask = 0;
            if x > start || start == end {
                mask |= DIR_W;
            }
            if x < end || start == end {
                mask |= DIR_E;
            }
            self.add_mask(x, y, mask);
        }
    }

    pub fn draw_v_line(&mut self, x: usize, y1: usize, y2: usize) {
        let (start, end) = (y1.min(y2), y1.max(y2));
        for y in start..=end {
            let mut mask = 0;
            if y > start || start == end {
                mask |= DIR_N;
            }
            if y < end || start == end {
                mask |= DIR_S;
            }
            self.add_mask(x, y, mask);
        }
    }

    pub fn draw_arrow(&mut self, x: usize, y: usize, dir: ArrowDir) {
        let ch = match dir {
            ArrowDir::Down => '▼',
            ArrowDir::Up => '▲',
            ArrowDir::Right => '▶',
            ArrowDir::Left => '◀',
        };
        self.put_char(x, y, ch, true);
    }

    pub fn draw_rounded_box(&mut self, x: usize, y: usize, w: usize, h: usize, lines: &[String]) {
        if w < 3 || h < 3 {
            return;
        }
        self.ensure_size(x + w, y + h);

        // Top border
        self.put_char(x, y, '╭', false);
        self.masks[y][x] = DIR_S | DIR_E;
        for col in (x + 1)..(x + w - 1) {
            self.put_char(col, y, '─', false);
            self.masks[y][col] = DIR_E | DIR_W;
        }
        self.put_char(x + w - 1, y, '╮', false);
        self.masks[y][x + w - 1] = DIR_S | DIR_W;

        // Sides
        for row in (y + 1)..(y + h - 1) {
            self.put_char(x, row, '│', false);
            self.masks[row][x] = DIR_N | DIR_S;
            for col in (x + 1)..(x + w - 1) {
                self.put_char(col, row, ' ', true);
            }
            self.put_char(x + w - 1, row, '│', false);
            self.masks[row][x + w - 1] = DIR_N | DIR_S;
        }

        // Bottom border
        self.put_char(x, y + h - 1, '╰', false);
        self.masks[y + h - 1][x] = DIR_N | DIR_E;
        for col in (x + 1)..(x + w - 1) {
            self.put_char(col, y + h - 1, '─', false);
            self.masks[y + h - 1][col] = DIR_E | DIR_W;
        }
        self.put_char(x + w - 1, y + h - 1, '╯', false);
        self.masks[y + h - 1][x + w - 1] = DIR_N | DIR_W;

        for (i, line) in lines.iter().enumerate() {
            if y + 1 + i < y + h - 1 {
                self.stamp_label(x + 1, y + 1 + i, w - 2, line);
            }
        }
    }

    pub fn draw_rect_box(&mut self, x: usize, y: usize, w: usize, h: usize, lines: &[String]) {
        if w < 3 || h < 3 {
            return;
        }
        self.ensure_size(x + w, y + h);

        self.put_char(x, y, '┌', false);
        self.masks[y][x] = DIR_S | DIR_E;
        for col in (x + 1)..(x + w - 1) {
            self.put_char(col, y, '─', false);
            self.masks[y][col] = DIR_E | DIR_W;
        }
        self.put_char(x + w - 1, y, '┐', false);
        self.masks[y][x + w - 1] = DIR_S | DIR_W;

        for row in (y + 1)..(y + h - 1) {
            self.put_char(x, row, '│', false);
            self.masks[row][x] = DIR_N | DIR_S;
            for col in (x + 1)..(x + w - 1) {
                self.put_char(col, row, ' ', true);
            }
            self.put_char(x + w - 1, row, '│', false);
            self.masks[row][x + w - 1] = DIR_N | DIR_S;
        }

        self.put_char(x, y + h - 1, '└', false);
        self.masks[y + h - 1][x] = DIR_N | DIR_E;
        for col in (x + 1)..(x + w - 1) {
            self.put_char(col, y + h - 1, '─', false);
            self.masks[y + h - 1][col] = DIR_E | DIR_W;
        }
        self.put_char(x + w - 1, y + h - 1, '┘', false);
        self.masks[y + h - 1][x + w - 1] = DIR_N | DIR_W;

        for (i, line) in lines.iter().enumerate() {
            if y + 1 + i < y + h - 1 {
                self.stamp_label(x + 1, y + 1 + i, w - 2, line);
            }
        }
    }

    pub fn draw_diamond_box(&mut self, x: usize, y: usize, w: usize, h: usize, lines: &[String]) {
        if w < 4 || h < 3 {
            return;
        }
        self.ensure_size(x + w, y + h);

        // Top angled roof: /───\
        self.put_char(x, y, ' ', true);
        self.put_char(x + 1, y, '/', true);
        for col in (x + 2)..(x + w - 2) {
            self.put_char(col, y, '─', true);
        }
        self.put_char(x + w - 2, y, '\\', true);
        self.put_char(x + w - 1, y, ' ', true);

        // Middle rows with diamond points: < line >
        for row in (y + 1)..(y + h - 1) {
            self.put_char(x, row, '<', true);
            for col in (x + 1)..(x + w - 1) {
                self.put_char(col, row, ' ', true);
            }
            self.put_char(x + w - 1, row, '>', true);
        }

        // Bottom angled floor: \───/
        let bot = y + h - 1;
        self.put_char(x, bot, ' ', true);
        self.put_char(x + 1, bot, '\\', true);
        for col in (x + 2)..(x + w - 2) {
            self.put_char(col, bot, '─', true);
        }
        self.put_char(x + w - 2, bot, '/', true);
        self.put_char(x + w - 1, bot, ' ', true);

        for (i, line) in lines.iter().enumerate() {
            if y + 1 + i < y + h - 1 {
                self.stamp_label(x + 1, y + 1 + i, w - 2, line);
            }
        }
    }

    pub fn draw_text(&mut self, x: usize, y: usize, text: &str) {
        let mut cur_x = x;
        for ch in text.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            self.put_char(cur_x, y, ch, true);
            cur_x += cw;
        }
    }

    fn stamp_label(&mut self, inner_x: usize, y: usize, inner_w: usize, label: &str) {
        let label_w = UnicodeWidthStr::width(label);
        let left_pad = inner_w.saturating_sub(label_w) / 2;
        let start_x = inner_x + left_pad;

        let mut cur_x = start_x;
        for ch in label.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            if cur_x + cw > inner_x + inner_w {
                break;
            }
            self.put_char(cur_x, y, ch, true);
            cur_x += cw;
        }
    }

    pub fn to_trimmed_string(&self) -> String {
        let mut lines: Vec<String> = Vec::with_capacity(self.height);
        for row in &self.chars {
            let line: String = row.iter().collect();
            lines.push(line.trim_end().to_string());
        }

        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }

        lines.join("\n")
    }
}

fn mask_to_glyph(mask: u8) -> char {
    match mask {
        m if m == (DIR_N | DIR_S | DIR_E | DIR_W) => '┼',
        m if m == (DIR_S | DIR_E | DIR_W) => '┬',
        m if m == (DIR_N | DIR_E | DIR_W) => '┴',
        m if m == (DIR_N | DIR_S | DIR_E) => '├',
        m if m == (DIR_N | DIR_S | DIR_W) => '┤',
        m if m == (DIR_S | DIR_E) => '╭',
        m if m == (DIR_S | DIR_W) => '╮',
        m if m == (DIR_N | DIR_E) => '╰',
        m if m == (DIR_N | DIR_W) => '╯',
        m if (m & (DIR_N | DIR_S)) != 0 => '│',
        m if (m & (DIR_E | DIR_W)) != 0 => '─',
        _ => '─',
    }
}
