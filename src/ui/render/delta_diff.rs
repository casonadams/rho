//! Delta-style diff highlighting (`ui.delta_diffs = true`).
//!
//! Combines `syntect` syntax highlighting (`CodeHighlighter`) with `similar`
//! line- and word-level diffs and subtle red/green background tints.

use super::diff::{DiffToken, compute_token_diff, format_gutter_prefix, more_lines_footer, replace_tabs, tokenize};
use crate::ui::block::leading_escape;
use crate::ui::markdown::CodeHighlighter;
use crate::ui::terminal_stream::is_sgr_reset;
use crate::ui::theme::Theme;
use std::ops::Range;

struct DeltaPalette {
    minus_bg: &'static str,
    minus_emph_bg: &'static str,
    plus_bg: &'static str,
    plus_emph_bg: &'static str,
}

fn palette_for_theme(theme: &Theme) -> DeltaPalette {
    if theme.is_light {
        DeltaPalette {
            minus_bg: "\x1b[48;2;255;230;230m",
            minus_emph_bg: "\x1b[48;2;255;192;192m",
            plus_bg: "\x1b[48;2;224;255;230m",
            plus_emph_bg: "\x1b[48;2;176;242;192m",
        }
    } else {
        DeltaPalette {
            minus_bg: "\x1b[48;2;58;24;28m",
            minus_emph_bg: "\x1b[48;2;104;34;40m",
            plus_bg: "\x1b[48;2;22;52;32m",
            plus_emph_bg: "\x1b[48;2;34;92;52m",
        }
    }
}

fn is_emphasized(pos: usize, ranges: &[Range<usize>]) -> bool {
    ranges.iter().any(|r| r.contains(&pos))
}

fn bg_for_state<'a>(emph: bool, base_bg: &'a str, emph_bg: &'a str) -> &'a str {
    if emph { emph_bg } else { base_bg }
}

fn overlay_diff_background(highlighted: &str, base_bg: &str, emph_bg: &str, emph_ranges: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(highlighted.len() + 32);
    let mut cur_emph = is_emphasized(0, emph_ranges);
    out.push_str(bg_for_state(cur_emph, base_bg, emph_bg));

    let mut offset = 0usize;
    let mut vis_pos = 0usize;
    while offset < highlighted.len() {
        if let Some((seq, _)) = leading_escape(&highlighted[offset..]) {
            out.push_str(seq);
            if is_sgr_reset(seq) {
                out.push_str(bg_for_state(cur_emph, base_bg, emph_bg));
            }
            offset += seq.len();
            continue;
        }
        let ch = highlighted[offset..].chars().next().unwrap_or_default();
        let want_emph = is_emphasized(vis_pos, emph_ranges);
        if want_emph != cur_emph {
            cur_emph = want_emph;
            out.push_str(bg_for_state(cur_emph, base_bg, emph_bg));
        }
        out.push(ch);
        offset += ch.len_utf8();
        vis_pos += ch.len_utf8();
    }
    out.push_str("\x1b[0m");
    out
}

fn emph_ranges_for_pair(clean_old: &str, clean_new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let old_tokens = tokenize(clean_old);
    let new_tokens = tokenize(clean_new);
    let diff = compute_token_diff(&old_tokens, &new_tokens);

    let mut old_ranges = Vec::new();
    let mut new_ranges = Vec::new();
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;
    for token in diff {
        match token {
            DiffToken::Same(text) => {
                old_pos += text.len();
                new_pos += text.len();
            }
            DiffToken::Removed(text) => {
                let end = old_pos + text.len();
                if !text.trim().is_empty() {
                    old_ranges.push(old_pos..end);
                }
                old_pos = end;
            }
            DiffToken::Added(text) => {
                let end = new_pos + text.len();
                if !text.trim().is_empty() {
                    new_ranges.push(new_pos..end);
                }
                new_pos = end;
            }
        }
    }
    (old_ranges, new_ranges)
}

#[derive(Clone, Copy)]
enum LineKind {
    Context,
    Removed,
    Added,
}

struct DeltaRenderCtx<'a> {
    theme: &'a Theme,
    palette: DeltaPalette,
    old_hl: CodeHighlighter,
    new_hl: CodeHighlighter,
    start_line: Option<usize>,
    gutter_width: usize,
    limit: usize,
    emitted: usize,
    hidden: usize,
}

impl<'a> DeltaRenderCtx<'a> {
    fn new(lang: Option<&str>, start_line: Option<usize>, gutter_width: usize, limit: usize, theme: &'a Theme) -> Self {
        Self {
            theme,
            palette: palette_for_theme(theme),
            old_hl: CodeHighlighter::new(lang),
            new_hl: CodeHighlighter::new(lang),
            start_line,
            gutter_width,
            limit,
            emitted: 0,
            hidden: 0,
        }
    }

    fn format_gutter(&self, line_offset: usize) -> String {
        match self.start_line {
            Some(start) => format_gutter_prefix(start + line_offset, self.gutter_width, self.theme.dimmed),
            None => String::new(),
        }
    }

    fn emit_line(&mut self, out: &mut String, kind: LineKind, line_offset: usize, raw: &str, emph: &[Range<usize>]) {
        let clean = replace_tabs(raw);
        let highlighted = match kind {
            LineKind::Removed => self.old_hl.highlight_line(&clean, self.theme),
            LineKind::Added => self.new_hl.highlight_line(&clean, self.theme),
            LineKind::Context => {
                let _ = self.old_hl.highlight_line(&clean, self.theme);
                self.new_hl.highlight_line(&clean, self.theme)
            }
        };
        if self.emitted >= self.limit {
            self.hidden += 1;
            return;
        }
        self.emitted += 1;
        let gutter = self.format_gutter(line_offset);
        match kind {
            LineKind::Context => {
                out.push_str(&format!("{gutter}  {highlighted}\n"));
            }
            LineKind::Removed => {
                let red = self.theme.tool_err;
                let bg = self.palette.minus_bg;
                let styled = overlay_diff_background(&highlighted, bg, self.palette.minus_emph_bg, emph);
                out.push_str(&format!("{gutter}{bg}{red}- {red:#}\x1b[0m{styled}\n"));
            }
            LineKind::Added => {
                let green = self.theme.tool_ok;
                let bg = self.palette.plus_bg;
                let styled = overlay_diff_background(&highlighted, bg, self.palette.plus_emph_bg, emph);
                out.push_str(&format!("{gutter}{bg}{green}+ {green:#}\x1b[0m{styled}\n"));
            }
        }
    }

    fn emit_replace(
        &mut self,
        out: &mut String,
        old_slice: &[&str],
        old_idx: usize,
        new_slice: &[&str],
        new_idx: usize,
    ) {
        if old_slice.len() == new_slice.len() {
            for (i, (old_line, new_line)) in old_slice.iter().zip(new_slice.iter()).enumerate() {
                let (old_emph, new_emph) = emph_ranges_for_pair(&replace_tabs(old_line), &replace_tabs(new_line));
                self.emit_line(out, LineKind::Removed, old_idx + i, old_line, &old_emph);
                self.emit_line(out, LineKind::Added, new_idx + i, new_line, &new_emph);
            }
        } else {
            for (i, line) in old_slice.iter().enumerate() {
                self.emit_line(out, LineKind::Removed, old_idx + i, line, &[]);
            }
            for (i, line) in new_slice.iter().enumerate() {
                self.emit_line(out, LineKind::Added, new_idx + i, line, &[]);
            }
        }
    }
}

pub(crate) struct DeltaDiffRequest<'a> {
    pub old_lines: &'a [&'a str],
    pub new_lines: &'a [&'a str],
    pub lang: Option<&'a str>,
    pub start_line: Option<usize>,
    pub gutter_width: usize,
    pub expanded: bool,
    pub theme: &'a Theme,
}

pub(crate) fn render_delta_diff(out: &mut String, req: DeltaDiffRequest<'_>) {
    let limit = if req.expanded { usize::MAX } else { 16 };
    let mut ctx = DeltaRenderCtx::new(req.lang, req.start_line, req.gutter_width, limit, req.theme);
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, req.old_lines, req.new_lines) {
        match op {
            similar::DiffOp::Equal { new_index, len, .. } => {
                for (i, line) in req.new_lines[new_index..new_index + len].iter().enumerate() {
                    ctx.emit_line(out, LineKind::Context, new_index + i, line, &[]);
                }
            }
            similar::DiffOp::Delete { old_index, old_len, .. } => {
                for (i, line) in req.old_lines[old_index..old_index + old_len].iter().enumerate() {
                    ctx.emit_line(out, LineKind::Removed, old_index + i, line, &[]);
                }
            }
            similar::DiffOp::Insert { new_index, new_len, .. } => {
                for (i, line) in req.new_lines[new_index..new_index + new_len].iter().enumerate() {
                    ctx.emit_line(out, LineKind::Added, new_index + i, line, &[]);
                }
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                ctx.emit_replace(
                    out,
                    &req.old_lines[old_index..old_index + old_len],
                    old_index,
                    &req.new_lines[new_index..new_index + new_len],
                    new_index,
                );
            }
        }
    }
    if ctx.hidden > 0 {
        out.push_str(&more_lines_footer(ctx.hidden, req.theme.dimmed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::strip_ansi;
    use crate::ui::render::diff::{EntryDiffInput, format_entry_diff};

    #[test]
    fn delta_diff_syntax_highlights_rust_and_adds_word_emphasis() {
        let theme = Theme {
            delta_diffs: true,
            ..Theme::default()
        };
        let rendered = format_entry_diff(EntryDiffInput {
            idx: 0,
            total: 1,
            expanded: true,
            old_text: "let count = 1;",
            new_text: "let count = 42;",
            theme: &theme,
            start_line: Some(10),
            lang: Some("rs"),
        });
        assert!(rendered.contains("\x1b[48;2;58;24;28m"), "minus bg:\n{rendered:?}");
        assert!(
            rendered.contains("\x1b[48;2;104;34;40m"),
            "minus emph bg:\n{rendered:?}"
        );
        assert!(rendered.contains("\x1b[48;2;22;52;32m"), "plus bg:\n{rendered:?}");
        assert!(rendered.contains("\x1b[48;2;34;92;52m"), "plus emph bg:\n{rendered:?}");
        assert!(
            rendered.contains("\x1b[35mlet"),
            "rust keyword syntax fg:\n{rendered:?}"
        );
        let plain = strip_ansi(&rendered);
        assert!(plain.contains(" 10 │ - let count = 1;"), "plain:\n{plain}");
        assert!(plain.contains(" 10 │ + let count = 42;"), "plain:\n{plain}");
    }

    #[test]
    fn delta_diff_preserves_equal_context_lines_in_multiline_edits() {
        let theme = Theme {
            delta_diffs: true,
            ..Theme::default()
        };
        let rendered = format_entry_diff(EntryDiffInput {
            idx: 0,
            total: 1,
            expanded: true,
            old_text: "fn a() {}\nlet x = 1;\nfn b() {}",
            new_text: "fn a() {}\nlet x = 2;\nfn b() {}",
            theme: &theme,
            start_line: Some(1),
            lang: Some("rs"),
        });
        let plain = strip_ansi(&rendered);
        let lines: Vec<&str> = plain.lines().collect();
        assert_eq!(lines.len(), 4, "got:\n{plain}");
        assert!(lines[0].contains("  fn a() {}"));
        assert!(lines[1].contains("- let x = 1;"));
        assert!(lines[2].contains("+ let x = 2;"));
        assert!(lines[3].contains("  fn b() {}"));
    }
}
