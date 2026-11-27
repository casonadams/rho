//! An agent's reply or thinking as it streams: markdown rendered inside the one-column margin,
//! the way the transcript shows it once committed. The main agent's renderer and the view of
//! another agent both stream through it, so the two read the same.

use super::margin::{Margin, margin_width};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Reply,
    Thinking,
}

pub(crate) struct StreamRenderer {
    kind: StreamKind,
    markdown: MarkdownRenderer,
    margin: Margin,
}

impl StreamRenderer {
    pub(crate) fn new(kind: StreamKind, theme: &Theme) -> Self {
        let style = match kind {
            StreamKind::Reply => None,
            StreamKind::Thinking => Some(theme.dimmed),
        };
        Self {
            kind,
            markdown: MarkdownRenderer::new(),
            margin: Margin::new(style),
        }
    }

    pub(crate) fn kind(&self) -> StreamKind {
        self.kind
    }

    /// Renders `token` for a terminal `width` columns wide; a line still being written is held back.
    pub(crate) fn push(&mut self, token: &str, width: usize, theme: &Theme) -> String {
        self.markdown.set_width(margin_width(width));
        let rendered = self.markdown.render_token(token, theme);
        self.margin.apply(&rendered)
    }

    /// Renders what is held back and starts over for the next block. Thinking ends without its last
    /// newline, which the turn writes after it.
    pub(crate) fn finish(&mut self, theme: &Theme) -> String {
        let rendered = self.markdown.flush(theme);
        let mut out = self.margin.apply(&rendered);
        if self.kind == StreamKind::Thinking && out.ends_with('\n') {
            out.pop();
        }
        *self = Self::new(self.kind, theme);
        out
    }
}
