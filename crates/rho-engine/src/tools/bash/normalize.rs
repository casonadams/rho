pub const BASH_CLEAN_WINDOW_LINE_THRESHOLD: usize = 50;
pub const BASH_CLEAN_HEAD_LINES: usize = 10;
pub const BASH_CLEAN_TAIL_LINES: usize = 15;
pub const BASH_FAILURE_TAIL_LINES: usize = 100;

pub fn collapse_consecutive_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let mut repeat_count = 1;
        while lines.peek() == Some(&line) {
            repeat_count += 1;
            lines.next();
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        if repeat_count >= 3 {
            use std::fmt::Write;
            let _ = write!(out, "\n[... {} identical lines collapsed]", repeat_count - 1);
        } else if repeat_count == 2 {
            out.push('\n');
            out.push_str(line);
        }
    }
    out
}

pub fn window_clean_output(text: &str, head_count: usize, tail_count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= head_count + tail_count + 5 {
        return text.to_string();
    }
    window_head_and_tail(&lines[..head_count].join("\n"), text, lines.len(), tail_count)
}

pub fn join_head_tail(head: &str, omitted: usize, tail: &str) -> String {
    format!("{head}\n\n[... {omitted} lines omitted]\n\n{tail}")
}

pub fn window_head_and_tail(head: &str, tail: &str, total_lines: usize, tail_count: usize) -> String {
    let head_lines = head.lines().count();
    let lines: Vec<&str> = tail.lines().collect();
    let keep = tail_count.min(total_lines.saturating_sub(head_lines)).min(lines.len());
    let shown = lines[lines.len() - keep..].join("\n");
    match total_lines.saturating_sub(head_lines + keep) {
        0 => format!("{head}\n{shown}"),
        omitted => join_head_tail(head, omitted, &shown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_consecutive_lines_no_repeats() {
        let input = "line 1\nline 2\nline 3";
        assert_eq!(collapse_consecutive_lines(input), input);
    }

    #[test]
    fn test_collapse_consecutive_lines_two_repeats() {
        let input = "line 1\nline 1\nline 2";
        assert_eq!(collapse_consecutive_lines(input), input);
    }

    #[test]
    fn test_collapse_consecutive_lines_many_repeats() {
        let input = "start\nrepeat\nrepeat\nrepeat\nrepeat\nend";
        let expected = "start\nrepeat\n[... 3 identical lines collapsed]\nend";
        assert_eq!(collapse_consecutive_lines(input), expected);
    }

    #[test]
    fn test_window_clean_output_short() {
        let input = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(window_clean_output(&input, 10, 15), input);
    }

    #[test]
    fn test_window_clean_output_long() {
        let input = (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = window_clean_output(&input, 10, 15);
        assert!(result.starts_with("line 1\nline 2"));
        assert!(result.contains("[... 75 lines omitted]"));
        assert!(result.ends_with("line 99\nline 100"));
    }

    #[test]
    fn a_tail_that_overlaps_the_head_shows_each_line_once() {
        let head = (1..=10).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let tail = (6..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let expected = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(window_head_and_tail(&head, &tail, 20, 15), expected);
    }
}
