use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::ir::{
    ChangeType, ContentBlock, DiagramKind, DiffHunk, HighlightedLine, InlineSpan, StyleToken, ToolInvocation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveStyle {
    Bold,
    Italic,
    Strikethrough,
    Link(String),
}

#[derive(Default)]
struct MarkdownParseState {
    blocks: Vec<ContentBlock>,
    current_inlines: Vec<InlineSpan>,
    current_heading_level: Option<u8>,
    in_code_block: Option<String>,
    code_accumulator: String,
    in_table: bool,
    table_headers: Vec<Vec<InlineSpan>>,
    table_rows: Vec<Vec<Vec<InlineSpan>>>,
    current_table_row: Vec<Vec<InlineSpan>>,
    current_cell: Vec<InlineSpan>,
    style_stack: Vec<ActiveStyle>,
}

impl MarkdownParseState {
    fn handle_start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.current_heading_level = Some(level as u8);
                self.current_inlines.clear();
            }
            Tag::Paragraph => self.current_inlines.clear(),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => l.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                self.in_code_block = Some(lang);
                self.code_accumulator.clear();
            }
            Tag::Table(_) => {
                self.in_table = true;
                self.table_headers.clear();
                self.table_rows.clear();
            }
            Tag::TableHead | Tag::TableRow => self.current_table_row.clear(),
            Tag::TableCell => self.current_cell.clear(),
            Tag::Emphasis => self.style_stack.push(ActiveStyle::Italic),
            Tag::Strong => self.style_stack.push(ActiveStyle::Bold),
            Tag::Strikethrough => self.style_stack.push(ActiveStyle::Strikethrough),
            Tag::Link { dest_url, .. } => self.style_stack.push(ActiveStyle::Link(dest_url.to_string())),
            _ => {}
        }
    }

    fn handle_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                if let Some(level) = self.current_heading_level.take() {
                    self.blocks.push(ContentBlock::Heading {
                        level,
                        content: std::mem::take(&mut self.current_inlines),
                    });
                }
            }
            TagEnd::Paragraph => {
                if !self.current_inlines.is_empty() {
                    self.blocks
                        .push(ContentBlock::Paragraph(std::mem::take(&mut self.current_inlines)));
                }
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::TableCell => self.current_table_row.push(std::mem::take(&mut self.current_cell)),
            TagEnd::TableHead => self.table_headers = std::mem::take(&mut self.current_table_row),
            TagEnd::TableRow => self.table_rows.push(std::mem::take(&mut self.current_table_row)),
            TagEnd::Table => {
                self.in_table = false;
                self.blocks.push(ContentBlock::Table {
                    headers: std::mem::take(&mut self.table_headers),
                    rows: std::mem::take(&mut self.table_rows),
                });
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                let _ = self.style_stack.pop();
            }
            _ => {}
        }
    }

    fn finish_code_block(&mut self) {
        let Some(lang) = self.in_code_block.take() else { return };
        if lang.eq_ignore_ascii_case("mermaid") {
            self.blocks.push(ContentBlock::Diagram {
                kind: DiagramKind::Mermaid,
                content: std::mem::take(&mut self.code_accumulator),
            });
        } else {
            let lines: Vec<HighlightedLine> = self
                .code_accumulator
                .lines()
                .map(|l| HighlightedLine {
                    spans: vec![InlineSpan::Text(l.to_string())],
                })
                .collect();
            self.blocks.push(ContentBlock::CodeFence {
                language: lang,
                content: std::mem::take(&mut self.code_accumulator),
                lines,
            });
        }
    }

    fn handle_text(&mut self, text: &str) {
        if self.in_code_block.is_some() {
            self.code_accumulator.push_str(text);
        } else {
            let span = apply_styles(text, &self.style_stack);
            if self.in_table {
                self.current_cell.push(span);
            } else {
                self.current_inlines.push(span);
            }
        }
    }

    fn handle_inline_code(&mut self, code: &str) {
        let span = InlineSpan::Code(code.to_string());
        if self.in_table {
            self.current_cell.push(span);
        } else {
            self.current_inlines.push(span);
        }
    }
}

pub fn parse_markdown(input: &str) -> Vec<ContentBlock> {
    let parser = Parser::new(input);
    let mut state = MarkdownParseState::default();

    for event in parser {
        match event {
            Event::Start(tag) => state.handle_start(tag),
            Event::End(tag) => state.handle_end(tag),
            Event::Text(text) => state.handle_text(&text),
            Event::Code(code) => state.handle_inline_code(&code),
            Event::SoftBreak => state.handle_text(" "),
            Event::HardBreak => state.handle_text("\n"),
            _ => {}
        }
    }

    if !state.current_inlines.is_empty() {
        state.blocks.push(ContentBlock::Paragraph(state.current_inlines));
    }

    state.blocks
}

fn apply_styles(text: &str, stack: &[ActiveStyle]) -> InlineSpan {
    let mut span = InlineSpan::Text(text.to_string());
    for style in stack.iter().rev() {
        let current_text = span.plain_text().to_string();
        span = match style {
            ActiveStyle::Bold => InlineSpan::Bold(current_text),
            ActiveStyle::Italic => InlineSpan::Italic(current_text),
            ActiveStyle::Strikethrough => InlineSpan::Strikethrough(current_text),
            ActiveStyle::Link(url) => InlineSpan::Link {
                label: current_text,
                url: url.clone(),
            },
        };
    }
    span
}

pub fn generate_diff(old_text: &str, new_text: &str, file_path: Option<String>) -> ContentBlock {
    let text_diff = similar::TextDiff::from_lines(old_text, new_text);
    let mut hunks = Vec::new();
    let mut old_line_num = 1;
    let mut new_line_num = 1;

    for change in text_diff.iter_all_changes() {
        let (change_type, old_line, new_line) = match change.tag() {
            similar::ChangeTag::Equal => {
                let res = (ChangeType::Equal, Some(old_line_num), Some(new_line_num));
                old_line_num += 1;
                new_line_num += 1;
                res
            }
            similar::ChangeTag::Delete => {
                let res = (ChangeType::Removed, Some(old_line_num), None);
                old_line_num += 1;
                res
            }
            similar::ChangeTag::Insert => {
                let res = (ChangeType::Added, None, Some(new_line_num));
                new_line_num += 1;
                res
            }
        };

        let line_content = change.value().trim_end_matches(['\r', '\n']).to_string();
        let inline_spans = vec![InlineSpan::Text(line_content.clone())];

        hunks.push(DiffHunk {
            change: change_type,
            old_line,
            new_line,
            line: line_content,
            inline_spans,
        });
    }

    ContentBlock::Diff {
        file_path,
        old_line: if hunks.is_empty() { None } else { Some(1) },
        new_line: if hunks.is_empty() { None } else { Some(1) },
        hunks,
    }
}

pub fn generate_word_diff(old_line: &str, new_line: &str) -> Vec<InlineSpan> {
    let diff = similar::TextDiff::from_words(old_line, new_line);
    let mut spans = Vec::new();

    for change in diff.iter_all_changes() {
        let text = change.value().to_string();
        match change.tag() {
            similar::ChangeTag::Equal => spans.push(InlineSpan::Text(text)),
            similar::ChangeTag::Delete => spans.push(InlineSpan::Styled {
                text,
                style: StyleToken::Error,
            }),
            similar::ChangeTag::Insert => spans.push(InlineSpan::Styled {
                text,
                style: StyleToken::Success,
            }),
        }
    }

    spans
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEvent {
    Token(String),
    ThinkingStarted,
    ThinkingDelta(String),
    ThinkingFinished {
        duration_ms: Option<u64>,
    },
    ToolStarted(ToolInvocation),
    ToolDelta {
        id: String,
        chunk: String,
    },
    ToolFinished {
        id: String,
        is_error: bool,
        output: String,
        duration_ms: Option<u64>,
    },
    Notice(String),
    StatusChanged(String),
    UsageUpdate {
        input_tokens: u64,
        output_tokens: u64,
        cache_read: u64,
        cache_write: u64,
        cost: Option<f64>,
    },
    TurnCompleted,
}

#[derive(Debug, Default)]
pub struct StreamChunkParser {
    in_thinking: bool,
    tag_prefix_buffer: String,
}

impl StreamChunkParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse_chunk(&mut self, chunk: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let mut full_input = std::mem::take(&mut self.tag_prefix_buffer);
        full_input.push_str(chunk);

        let mut remainder = full_input.as_str();

        while !remainder.is_empty() {
            if !self.in_thinking {
                if let Some(start_idx) = remainder.find("<thinking>") {
                    let before = &remainder[..start_idx];
                    if !before.is_empty() {
                        events.push(StreamEvent::Token(before.to_string()));
                    }
                    self.in_thinking = true;
                    events.push(StreamEvent::ThinkingStarted);
                    remainder = &remainder[start_idx + "<thinking>".len()..];
                } else if let Some(prefix_idx) = remainder.rfind('<') {
                    if "<thinking>".starts_with(&remainder[prefix_idx..]) {
                        let before = &remainder[..prefix_idx];
                        if !before.is_empty() {
                            events.push(StreamEvent::Token(before.to_string()));
                        }
                        self.tag_prefix_buffer = remainder[prefix_idx..].to_string();
                        break;
                    } else {
                        events.push(StreamEvent::Token(remainder.to_string()));
                        break;
                    }
                } else {
                    events.push(StreamEvent::Token(remainder.to_string()));
                    break;
                }
            } else if let Some(end_idx) = remainder.find("</thinking>") {
                let thought = &remainder[..end_idx];
                if !thought.is_empty() {
                    events.push(StreamEvent::ThinkingDelta(thought.to_string()));
                }
                self.in_thinking = false;
                events.push(StreamEvent::ThinkingFinished { duration_ms: None });
                remainder = &remainder[end_idx + "</thinking>".len()..];
            } else if let Some(prefix_idx) = remainder.rfind('<') {
                if "</thinking>".starts_with(&remainder[prefix_idx..]) {
                    let thought = &remainder[..prefix_idx];
                    if !thought.is_empty() {
                        events.push(StreamEvent::ThinkingDelta(thought.to_string()));
                    }
                    self.tag_prefix_buffer = remainder[prefix_idx..].to_string();
                    break;
                } else {
                    events.push(StreamEvent::ThinkingDelta(remainder.to_string()));
                    break;
                }
            } else {
                events.push(StreamEvent::ThinkingDelta(remainder.to_string()));
                break;
            }
        }

        events
    }

    pub fn flush(&mut self) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let remaining = std::mem::take(&mut self.tag_prefix_buffer);
        if !remaining.is_empty() {
            if self.in_thinking {
                events.push(StreamEvent::ThinkingDelta(remaining));
            } else {
                events.push(StreamEvent::Token(remaining));
            }
        }
        if self.in_thinking {
            self.in_thinking = false;
            events.push(StreamEvent::ThinkingFinished { duration_ms: None });
        }
        events
    }
}
