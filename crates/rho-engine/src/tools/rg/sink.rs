use crate::tools::truncate::truncate_line;
use grep_searcher::{Searcher, Sink, SinkContext, SinkContextKind, SinkMatch};

/// One line of a match block: `:` marks a matching line, `-` a context line (ripgrep's convention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitLine {
    pub number: u64,
    pub marker: char,
    pub text: String,
}

/// A match with the context lines that belong to it. `detached` is set when lines were skipped
/// between this block and the previous one in the same file.
#[derive(Debug, Clone, Default)]
pub struct MatchBlock {
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
pub struct BlockSink {
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
