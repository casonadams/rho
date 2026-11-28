use crate::tools::truncate::truncate_line;
use grep_searcher::{Searcher, Sink, SinkContext, SinkContextKind, SinkMatch};

/// One line of a match block: `:` marks a matching line, `-` a context line (ripgrep's convention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HitLine {
    pub number: u64,
    pub marker: char,
    pub text: String,
}

/// A match with the context lines that belong to it. `detached` is set when lines were skipped
/// between this block and the previous one in the same file.
#[derive(Debug, Clone, Default)]
pub(crate) struct MatchBlock {
    pub lines: Vec<HitLine>,
    pub detached: bool,
    pub truncated: bool,
}

impl MatchBlock {
    pub fn first_line(&self) -> u64 {
        self.lines
            .iter()
            .find(|line| line.marker == ':')
            .map_or(0, |line| line.number)
    }
}

/// Collects a file's matches as blocks; context lines join the match they precede or follow.
pub(crate) struct BlockSink {
    pub keep_lines: bool,
    pub stop_at_first: bool,
    pub count: usize,
    pub blocks: Vec<MatchBlock>,
    pending_before: Vec<HitLine>,
    pending_break: bool,
    truncated_pending: bool,
}

impl BlockSink {
    pub fn new(keep_lines: bool, stop_at_first: bool) -> Self {
        Self {
            keep_lines,
            stop_at_first,
            count: 0,
            blocks: Vec::new(),
            pending_before: Vec::new(),
            pending_break: false,
            truncated_pending: false,
        }
    }

    fn hit_lines(&mut self, bytes: &[u8], first: u64, marker: char) -> Vec<HitLine> {
        let text = String::from_utf8_lossy(bytes);
        let text = text.strip_suffix('\n').unwrap_or(&text);
        text.split('\n')
            .zip(first..)
            .map(|(line, number)| {
                let cut = truncate_line(line.trim_end_matches('\r'));
                self.truncated_pending |= cut.was_truncated;
                HitLine {
                    number,
                    marker,
                    text: cut.text.into_owned(),
                }
            })
            .collect()
    }
}

impl Sink for BlockSink {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, found: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.count += 1;
        if self.keep_lines {
            let mut lines = std::mem::take(&mut self.pending_before);
            let matched = self.hit_lines(found.bytes(), found.line_number().unwrap_or(0), ':');
            lines.extend(matched);
            let detached = std::mem::take(&mut self.pending_break) && !self.blocks.is_empty();
            let truncated = std::mem::take(&mut self.truncated_pending);
            self.blocks.push(MatchBlock {
                lines,
                detached,
                truncated,
            });
        }
        Ok(!self.stop_at_first)
    }

    fn context(&mut self, _: &Searcher, context: &SinkContext<'_>) -> Result<bool, Self::Error> {
        let lines = self.hit_lines(context.bytes(), context.line_number().unwrap_or(0), '-');
        match (context.kind(), self.blocks.last_mut()) {
            (SinkContextKind::After, Some(block)) => {
                block.lines.extend(lines);
                block.truncated |= std::mem::take(&mut self.truncated_pending);
            }
            _ => self.pending_before.extend(lines),
        }
        Ok(true)
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, Self::Error> {
        self.pending_break = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grep_regex::RegexMatcher;
    use grep_searcher::SearcherBuilder;

    fn run(content: &str, context: usize, stop_at_first: bool) -> BlockSink {
        let matcher = RegexMatcher::new("needle").unwrap();
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .before_context(context)
            .after_context(context)
            .build();
        let mut sink = BlockSink::new(true, stop_at_first);
        searcher.search_slice(&matcher, content.as_bytes(), &mut sink).unwrap();
        sink
    }

    fn numbers(block: &MatchBlock) -> Vec<(u64, char)> {
        block.lines.iter().map(|l| (l.number, l.marker)).collect()
    }

    #[test]
    fn a_gap_between_blocks_marks_only_the_later_block_as_detached() {
        let content = format!("needle\n{}needle\n", "x\n".repeat(10));
        let sink = run(&content, 1, false);
        let detached: Vec<bool> = sink.blocks.iter().map(|b| b.detached).collect();
        assert_eq!(detached, [false, true]);
        assert_eq!(numbers(&sink.blocks[1]), [(11, '-'), (12, ':')]);
    }

    #[test]
    fn blocks_whose_context_touches_are_not_detached() {
        let sink = run("needle\nx\nneedle\n", 1, false);
        assert_eq!(sink.blocks.len(), 2);
        assert!(sink.blocks.iter().all(|b| !b.detached));
    }

    #[test]
    fn a_cut_line_marks_the_block_that_shows_it_as_truncated() {
        let long = "y".repeat(crate::tools::truncate::GREP_MAX_LINE_LENGTH + 10);
        let before = run(&format!("{long}\nneedle\n"), 1, false);
        let matched = run(&format!("needle {long}\n"), 0, false);
        let after = run(&format!("needle\n{long}\n"), 1, false);
        let plain = run("x\nneedle\ny\n", 1, false);
        let flags = [&before, &matched, &after, &plain].map(|sink| sink.blocks[0].truncated);
        assert_eq!(flags, [true, true, true, false]);
        assert!(before.blocks[0].lines[0].text.ends_with("... [truncated]"));
    }

    #[test]
    fn crlf_line_endings_are_stripped_from_matches_and_context() {
        let sink = run("before\r\nneedle\r\nafter\r\n", 1, false);
        let texts: Vec<&str> = sink.blocks[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["before", "needle", "after"]);
    }

    #[test]
    fn stopping_at_the_first_match_keeps_its_leading_context_and_ignores_later_matches() {
        let sink = run("a\nb\nneedle\nc\nneedle\n", 2, true);
        assert_eq!(sink.count, 1);
        assert_eq!(sink.blocks.len(), 1);
        assert_eq!(numbers(&sink.blocks[0]), [(1, '-'), (2, '-'), (3, ':')]);
    }
}
