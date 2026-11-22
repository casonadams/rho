const MAX_COMMAND_CHARS: usize = 40;

/// What a session header shows besides the screen itself.
pub struct SessionHeader<'a> {
    pub id: u32,
    pub command: &'a str,
    pub cols: u16,
    pub rows: u16,
    /// Zero-based (row, column), as the emulator reports it.
    pub cursor: (u16, u16),
    pub status: &'a str,
}

impl SessionHeader<'_> {
    /// `[terminal 1: nvim 120x40, cursor 5:12, running]`, with a one-based cursor.
    pub fn line(&self) -> String {
        let command = if self.command.chars().count() > MAX_COMMAND_CHARS {
            let head: String = self.command.chars().take(MAX_COMMAND_CHARS - 3).collect();
            format!("{head}...")
        } else {
            self.command.to_string()
        };
        format!(
            "[terminal {}: {command} {}x{}, cursor {}:{}, {}]",
            self.id,
            self.cols,
            self.rows,
            self.cursor.0 + 1,
            self.cursor.1 + 1,
            self.status
        )
    }
}

/// The header, any scrollback lines (oldest first), then each screen row as `N|text`, without
/// trailing spaces or trailing blank rows.
pub fn format_screen(header: &SessionHeader<'_>, scrollback: &[String], rows: &[String]) -> String {
    let trimmed: Vec<&str> = rows.iter().map(|row| row.trim_end()).collect();
    let shown = trimmed
        .iter()
        .rposition(|row| !row.is_empty())
        .map_or(0, |last| last + 1);
    let width = shown.to_string().len();
    let mut out = header.line();
    if !scrollback.is_empty() {
        out.push_str(&format!("\nscrollback (last {} lines):", scrollback.len()));
        for line in scrollback {
            out.push_str(&format!("\n-|{}", line.trim_end()));
        }
        out.push_str("\nscreen:");
    }
    for (index, row) in trimmed[..shown].iter().enumerate() {
        out.push_str(&format!("\n{:>width$}|{row}", index + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(command: &str) -> SessionHeader<'_> {
        SessionHeader {
            id: 3,
            command,
            cols: 120,
            rows: 40,
            cursor: (4, 11),
            status: "running",
        }
    }

    #[test]
    fn header_shows_size_one_based_cursor_and_status() {
        assert_eq!(header("nvim").line(), "[terminal 3: nvim 120x40, cursor 5:12, running]");
    }

    #[test]
    fn header_shortens_long_commands() {
        let line = header(&"x".repeat(60)).line();
        assert!(
            line.starts_with(&format!("[terminal 3: {}... 120x40", "x".repeat(37))),
            "{line}"
        );
    }

    #[test]
    fn screen_numbers_rows_and_drops_trailing_blanks() {
        let rows: Vec<String> = ["a  ", "", "b"]
            .into_iter()
            .chain(std::iter::repeat_n("", 8))
            .chain(["c   "])
            .chain(["   ", ""])
            .map(String::from)
            .collect();
        let screen = format_screen(&header("vim"), &[], &rows);
        let expected = "[terminal 3: vim 120x40, cursor 5:12, running]\n 1|a\n 2|\n 3|b\n 4|\n 5|\n 6|\n 7|\n 8|\n 9|\n10|\n11|\n12|c";
        assert_eq!(screen, expected);
    }

    #[test]
    fn blank_screen_is_only_the_header() {
        let rows = vec![String::new(); 5];
        assert_eq!(format_screen(&header("cat"), &[], &rows), header("cat").line());
    }

    #[test]
    fn scrollback_is_labelled_above_the_screen() {
        let rows = vec!["a".to_string(), String::new()];
        let scrollback = vec!["x  ".to_string(), "y".to_string()];
        let screen = format_screen(&header("seq"), &scrollback, &rows);
        let expected = format!(
            "{}\nscrollback (last 2 lines):\n-|x\n-|y\nscreen:\n1|a",
            header("seq").line()
        );
        assert_eq!(screen, expected);
    }
}
