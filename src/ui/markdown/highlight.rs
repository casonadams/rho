//! Transparent, theme-native syntax highlighting via `syntect` scope parsing.
//!
//! Instead of baking a hardcoded 24-bit `syntect` color scheme (like
//! `base16-ocean`), this parses lines into TextMate scopes and maps semantic
//! token roles onto the active [`Theme`] and standard ANSI-16 palette slots.
//! Unstyled text, identifiers, and punctuation emit no color escapes so they
//! inherit the terminal's native foreground transparently.

use crate::ui::theme::{SGR_RESET, Theme};
use anstyle::{AnsiColor, Color, Style};
use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::LazyLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static SCOPE_RULES: LazyLock<Vec<(Scope, TokenRole)>> = LazyLock::new(build_scope_rules);
static BOLD_SCOPE: LazyLock<Scope> = LazyLock::new(|| Scope::new("markup.bold").expect("valid scope"));
static ITALIC_SCOPE: LazyLock<Scope> = LazyLock::new(|| Scope::new("markup.italic").expect("valid scope"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenRole {
    Plain,
    Comment,
    Keyword,
    String,
    Number,
    Function,
    Type,
    Inserted,
    Deleted,
    Heading,
    Raw,
    Link,
    Quote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenStyle {
    role: TokenRole,
    bold: bool,
    italic: bool,
}

impl TokenRole {
    fn style(self, theme: &Theme) -> Option<Style> {
        match self {
            Self::Plain => None,
            Self::Comment | Self::Quote => Some(theme.dimmed),
            Self::Keyword => Some(ansi_fg(AnsiColor::Magenta)),
            Self::String => Some(ansi_fg(AnsiColor::Green)),
            Self::Number => Some(ansi_fg(AnsiColor::Yellow)),
            Self::Function => Some(ansi_fg(AnsiColor::Blue)),
            Self::Type => Some(ansi_fg(AnsiColor::Cyan)),
            Self::Inserted => Some(theme.tool_ok),
            Self::Deleted => Some(theme.tool_err),
            Self::Heading => Some(theme.heading_h1.bold()),
            Self::Raw => Some(theme.code_inline),
            Self::Link => Some(theme.highlight),
        }
    }
}

impl TokenStyle {
    fn style(self, theme: &Theme) -> Option<Style> {
        let base = self.role.style(theme);
        if base.is_none() && !self.bold && !self.italic {
            return None;
        }
        let mut style = base.unwrap_or_default();
        if self.bold {
            style = style.bold();
        }
        if self.italic {
            style = style.italic();
        }
        Some(style)
    }
}

fn ansi_fg(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(color)))
}

fn build_scope_rules() -> Vec<(Scope, TokenRole)> {
    const RULES: &[(&str, TokenRole)] = &[
        ("comment", TokenRole::Comment),
        ("markup.inserted", TokenRole::Inserted),
        ("markup.deleted", TokenRole::Deleted),
        ("meta.diff.header", TokenRole::Keyword),
        ("meta.diff.range", TokenRole::Keyword),
        ("markup.heading", TokenRole::Heading),
        ("entity.name.section", TokenRole::Heading),
        ("markup.raw", TokenRole::Raw),
        ("markup.underline.link", TokenRole::Link),
        ("meta.link", TokenRole::Link),
        ("markup.quote", TokenRole::Quote),
        ("string", TokenRole::String),
        ("constant.character", TokenRole::String),
        ("meta.string", TokenRole::String),
        ("constant.numeric", TokenRole::Number),
        ("constant.integer", TokenRole::Number),
        ("keyword.operator", TokenRole::Plain),
        ("keyword", TokenRole::Keyword),
        ("storage.type", TokenRole::Keyword),
        ("storage.modifier", TokenRole::Keyword),
        ("entity.name.function", TokenRole::Function),
        ("support.function", TokenRole::Function),
        ("variable.function", TokenRole::Function),
        ("entity.name.type", TokenRole::Type),
        ("support.type", TokenRole::Type),
        ("support.class", TokenRole::Type),
        ("entity.name.class", TokenRole::Type),
        ("entity.name.struct", TokenRole::Type),
        ("entity.name.enum", TokenRole::Type),
        ("entity.name.interface", TokenRole::Type),
        ("entity.name.trait", TokenRole::Type),
        ("constant.other.language-name", TokenRole::Raw),
        ("constant", TokenRole::Number),
        // Embedded source scopes (e.g. `source.rust` inside markdown fences)
        // stop outer `markup.raw.block` rules from tinting plain code tokens.
        ("source", TokenRole::Plain),
    ];
    RULES
        .iter()
        .filter_map(|&(pattern, role)| Scope::new(pattern).ok().map(|scope| (scope, role)))
        .collect()
}

fn classify_scope(scope: Scope) -> Option<TokenRole> {
    SCOPE_RULES
        .iter()
        .find(|(prefix, _)| prefix.is_prefix_of(scope))
        .map(|(_, role)| *role)
}

fn resolve_style(stack: &ScopeStack) -> TokenStyle {
    let mut role = None;
    let mut bold = false;
    let mut italic = false;
    for &scope in stack.as_slice().iter().rev() {
        if BOLD_SCOPE.is_prefix_of(scope) {
            bold = true;
        }
        if ITALIC_SCOPE.is_prefix_of(scope) {
            italic = true;
        }
        if role.is_none() {
            role = classify_scope(scope);
        }
    }
    TokenStyle {
        role: role.unwrap_or(TokenRole::Plain),
        bold,
        italic,
    }
}

fn resolve_syntax(lang: Option<&str>) -> &'static SyntaxReference {
    let ss = &*SYNTAX_SET;
    lang.and_then(|l| ss.find_syntax_by_token(l).or_else(|| ss.find_syntax_by_extension(l)))
        .or_else(|| {
            lang.filter(|l| is_typescript(l))
                .and_then(|_| ss.find_syntax_by_extension("js"))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// The bundled grammars have no TypeScript, so it borrows JavaScript's.
fn is_typescript(lang: &str) -> bool {
    ["ts", "tsx", "mts", "cts", "typescript"]
        .iter()
        .any(|t| lang.eq_ignore_ascii_case(t))
}

fn emit_segment(out: &mut String, text: &str, style: TokenStyle, theme: &Theme) {
    if text.is_empty() {
        return;
    }
    if let Some(style) = style.style(theme) {
        let _ = write!(out, "{style}{text}{style:#}");
    } else {
        out.push_str(text);
    }
}

struct RunEmitter<'a> {
    line: &'a str,
    theme: &'a Theme,
    out: String,
    run_start: usize,
    run_style: Option<TokenStyle>,
}

impl<'a> RunEmitter<'a> {
    fn new(line: &'a str, theme: &'a Theme) -> Self {
        Self {
            line,
            theme,
            out: String::with_capacity(line.len() + 32),
            run_start: 0,
            run_style: None,
        }
    }

    fn push(&mut self, seg_start: usize, next_style: TokenStyle) {
        if let Some(prev_style) = self.run_style {
            if prev_style != next_style {
                emit_segment(
                    &mut self.out,
                    &self.line[self.run_start..seg_start],
                    prev_style,
                    self.theme,
                );
                self.run_start = seg_start;
                self.run_style = Some(next_style);
            }
        } else {
            self.run_start = seg_start;
            self.run_style = Some(next_style);
        }
    }

    fn finish(mut self) -> String {
        if let Some(style) = self.run_style {
            emit_segment(&mut self.out, &self.line[self.run_start..], style, self.theme);
        }
        self.out
    }
}

pub struct CodeHighlighter {
    lang: Option<String>,
    parse_state: ParseState,
    scope_stack: ScopeStack,
}

fn is_valid_sgr_params(params: &str) -> bool {
    params.bytes().all(|b| b.is_ascii_digit() || b == b';')
}

fn parse_escaped_sgr(rest: &str) -> Option<(&str, usize)> {
    let prefix_len = if rest.starts_with("\\x1b[") || rest.starts_with("\\033[") {
        5
    } else if rest.starts_with("\\e[") {
        3
    } else if rest.starts_with("\\u001b[") {
        7
    } else if rest.starts_with("\x1b[") {
        2
    } else {
        return None;
    };
    let after = &rest[prefix_len..];
    let m_pos = after.find('m')?;
    let params = &after[..m_pos];
    is_valid_sgr_params(params).then_some((params, prefix_len + m_pos + 1))
}

fn render_ansi_code_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut emitted_sgr = false;
    while !rest.is_empty() {
        if let Some((params, consumed)) = parse_escaped_sgr(rest) {
            out.push_str("\x1b[");
            out.push_str(params);
            out.push('m');
            rest = &rest[consumed..];
            emitted_sgr = true;
            continue;
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    if emitted_sgr && !out.ends_with(SGR_RESET) && !out.ends_with("\x1b[m") {
        out.push_str(SGR_RESET);
    }
    out
}

impl CodeHighlighter {
    pub fn new(lang: Option<&str>) -> Self {
        let syntax = resolve_syntax(lang);
        Self {
            lang: lang.map(str::to_string),
            parse_state: ParseState::new(syntax),
            scope_stack: ScopeStack::new(),
        }
    }

    pub fn lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }

    pub fn highlight_line(&mut self, line: &str, theme: &Theme) -> String {
        if self.lang.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("ansi")) {
            return render_ansi_code_line(line);
        }
        let parse_input: Cow<'_, str> = if line.ends_with('\n') {
            Cow::Borrowed(line)
        } else {
            Cow::Owned(format!("{line}\n"))
        };
        let Ok(ops) = self.parse_state.parse_line(&parse_input, &SYNTAX_SET) else {
            return line.to_string();
        };

        let visible_len = line.len();
        let mut emitter = RunEmitter::new(line, theme);
        let mut prev_end = 0usize;

        for (offset, op) in ops {
            let seg_start = prev_end.min(visible_len);
            let seg_end = offset.min(visible_len);
            if seg_end > seg_start {
                emitter.push(seg_start, resolve_style(&self.scope_stack));
            }
            prev_end = offset;
            let _ = self.scope_stack.apply(&op);
        }

        if prev_end < visible_len {
            emitter.push(prev_end, resolve_style(&self.scope_stack));
        }
        emitter.finish()
    }
}

pub fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> String {
    CodeHighlighter::new(lang).highlight_line(line, theme)
}

/// Highlights each line of `text` as `lang`, carrying parser state across lines.
pub fn highlight_text(text: &str, lang: Option<&str>, theme: &Theme) -> String {
    let mut highlighter = CodeHighlighter::new(lang);
    text.lines()
        .map(|line| highlighter.highlight_line(line, theme))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Highlights the lines between ``` fences by the fence's language and dims the fences; lines
/// outside a fence are left as they are.
pub fn highlight_fenced_blocks(text: &str, theme: &Theme) -> String {
    highlight_fenced_blocks_in(text, None, theme)
}

/// Like `highlight_fenced_blocks`, for text that starts inside the fence `open` highlights.
pub fn highlight_fenced_blocks_in(text: &str, open: Option<CodeHighlighter>, theme: &Theme) -> String {
    let dim = theme.dimmed;
    let mut active = open;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(info) = line.trim_start().strip_prefix("```") {
            active = match active {
                Some(_) => None,
                None => Some(CodeHighlighter::new(info.split_whitespace().next())),
            };
            out.push(format!("{dim}{line}{dim:#}"));
        } else if let Some(highlighter) = active.as_mut() {
            out.push(highlighter.highlight_line(line, theme));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

pub fn warmup_syntax_highlighter() {
    let _ = &*SYNTAX_SET;
    let _ = &*SCOPE_RULES;
    let _ = &*BOLD_SCOPE;
    let _ = &*ITALIC_SCOPE;
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn ansi_code_block_renders_escaped_sgr_and_resets() {
        let theme = Theme::default();
        let line = r"\x1b[32m✓\x1b[0m \e[1mBash\033[0m \x1b[2m3.4s";
        let rendered = highlight_code_line(line, Some("ansi"), &theme);
        assert_eq!(rendered, "\x1b[32m✓\x1b[0m \x1b[1mBash\x1b[0m \x1b[2m3.4s\x1b[0m");
    }

    #[test]
    fn ansi_code_block_ignores_non_sgr_escape_sequences() {
        let theme = Theme::default();
        let line = r"\x1b[2Jclear";
        let rendered = highlight_code_line(line, Some("ansi"), &theme);
        assert_eq!(rendered, r"\x1b[2Jclear");
    }

    #[test_case("ts"; "ts")]
    #[test_case("tsx"; "tsx")]
    #[test_case("TS"; "uppercase ts")]
    fn typescript_extensions_are_highlighted_like_javascript(ext: &str) {
        let theme = Theme::default();
        let line = "const x = 1; // note";
        let js = highlight_code_line(line, Some("js"), &theme);
        assert!(js.contains("\x1b["));
        assert_eq!(highlight_code_line(line, Some(ext), &theme), js);
    }

    #[test]
    fn fenced_blocks_highlight_only_inside_fences_and_dim_the_fences() {
        let theme = Theme::default();
        let text = "before let\n```rust\nlet x = 1;\n```\nafter let";
        let out = highlight_fenced_blocks(text, &theme);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "before let");
        assert_eq!(lines[1], format!("{}```rust{:#}", theme.dimmed, theme.dimmed));
        assert!(lines[2].contains("\x1b[35mlet\x1b[0m"), "got: {:?}", lines[2]);
        assert_eq!(lines[4], "after let");
    }

    #[test]
    fn highlight_text_carries_state_across_lines() {
        let theme = Theme::default();
        let out = highlight_text("/* open\nstill comment */", Some("rust"), &theme);
        let dimmed = format!("{}", theme.dimmed);
        assert!(out.lines().all(|line| line.contains(&dimmed)), "got: {out:?}");
    }

    #[test]
    fn plain_text_emits_no_ansi_escapes() {
        let theme = Theme::default();
        let highlighted = highlight_code_line("just plain text", None, &theme);
        assert_eq!(highlighted, "just plain text");
    }

    #[test]
    fn code_uses_normal_ansi_palette_without_bright_or_truecolor() {
        let theme = Theme::default();
        let highlighted = highlight_code_line("let count = 42; // note", Some("rust"), &theme);
        assert!(highlighted.contains("\x1b[35mlet\x1b[0m"), "got: {highlighted:?}");
        assert!(highlighted.contains("\x1b[33m42\x1b[0m"), "got: {highlighted:?}");
        assert!(!highlighted.contains("\x1b[9"), "no bright ANSI codes: {highlighted:?}");
        assert!(
            !highlighted.contains("\x1b[37m"),
            "no forced white ANSI: {highlighted:?}"
        );
        assert!(
            !highlighted.contains("\x1b[48;"),
            "no background fills: {highlighted:?}"
        );
    }

    #[test]
    fn single_line_comment_does_not_leak_to_next_line() {
        let theme = Theme::default();
        let mut hl = CodeHighlighter::new(Some("rust"));
        let first = hl.highlight_line("// comment", &theme);
        let second = hl.highlight_line("let x = 1;", &theme);
        assert!(first.contains(&format!("{}", theme.dimmed)), "first: {first:?}");
        assert!(second.contains("\x1b[35mlet\x1b[0m"), "second: {second:?}");
    }

    #[test]
    fn markdown_uses_theme_styles_and_transparent_body() {
        let theme = Theme::default();
        let mut hl = CodeHighlighter::new(Some("md"));

        let heading = hl.highlight_line("# Heading", &theme);
        assert_eq!(
            heading,
            format!("{}# Heading{:#}", theme.heading_h1.bold(), theme.heading_h1.bold())
        );

        let body = hl.highlight_line("Plain body text", &theme);
        assert_eq!(body, "Plain body text");

        let bold = hl.highlight_line("Some **bold** word", &theme);
        assert!(bold.contains("\x1b[1m"), "bold: {bold:?}");
        assert!(!bold.contains("\x1b[93m"), "bold must not be bright yellow: {bold:?}");
        let both = hl.highlight_line("Some ***both*** word", &theme);
        let expected = Style::new().bold().italic();
        assert!(
            both.contains(&format!("{expected}*both*{expected:#}")),
            "bold+italic: {both:?}"
        );
    }

    #[test]
    fn python_call_arguments_and_punctuation_stay_unstyled() {
        let theme = Theme::default();
        let highlighted = highlight_code_line("f(value)", Some("python"), &theme);
        assert_eq!(highlighted, "\x1b[34mf\x1b[0m(value)");
    }

    #[test]
    fn markdown_inline_code_and_fences_use_theme_code_style_without_bright_ansi() {
        let theme = Theme::default();
        let mut hl = CodeHighlighter::new(Some("md"));
        let inline = hl.highlight_line("Run `cargo test` now", &theme);
        assert!(inline.contains("`cargo test`"), "inline: {inline:?}");
        assert!(inline.contains(&format!("{}", theme.code_inline)), "inline: {inline:?}");
        assert!(!inline.contains("\x1b[9"), "no bright ANSI codes: {inline:?}");

        let fence = hl.highlight_line("```rust", &theme);
        assert_eq!(fence, format!("{}```rust{:#}", theme.code_inline, theme.code_inline));
        assert!(!fence.contains("\x1b[33m"), "fence lang must not be yellow: {fence:?}");

        let code = hl.highlight_line("let x = 1;", &theme);
        assert!(code.contains("let x = 1;"), "code: {code:?}");
        assert!(!code.contains("\x1b[9"), "no bright ANSI codes: {code:?}");
        assert!(!code.contains("\x1b[37m"), "no forced white ANSI: {code:?}");
    }
}
