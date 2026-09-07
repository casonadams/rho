//! Command analysis for permission evaluation: normalizes tokenized commands,
//! extracts path arguments, and formats compound commands for display.

use super::lexer::{Token, TokenKind, tokenize};

// ---------------------------------------------------------------------------
// Token filtering: wrapper stripping, assignments, and path extraction
// ---------------------------------------------------------------------------

const WRAPPERS: &[&str] = &["time", "nice", "nohup", "command", "builtin", "noglob"];

pub(crate) fn extract_redirect_targets(words: &[Token], path_tokens: &mut Vec<String>) {
    for i in 0..words.len() {
        if words[i].kind != TokenKind::Redirect {
            continue;
        }
        if let Some(target) = words.get(i + 1)
            && target.kind == TokenKind::Word
            && !is_fd_dup_target(&target.text)
            && !path_tokens.contains(&target.text)
        {
            path_tokens.push(target.text.clone());
        }
    }
}

fn is_fd_dup_target(text: &str) -> bool {
    text.starts_with('&') && text[1..].chars().all(|c| c.is_ascii_digit())
}

fn is_fd_dup_token(raw: &str) -> bool {
    let s = raw.trim_start_matches(|c: char| c.is_ascii_digit());
    s.starts_with(">&") && s[2..].chars().all(|c| c.is_ascii_digit())
}

pub fn has_file_redirection(command: &str) -> bool {
    let token_res = tokenize(command);
    let tokens = &token_res.tokens;
    for i in 0..tokens.len() {
        if tokens[i].kind == TokenKind::Redirect {
            if is_fd_dup_token(&tokens[i].raw) {
                continue;
            }
            if let Some(target) = tokens.get(i + 1)
                && target.kind == TokenKind::Word
                && is_fd_dup_target(&target.text)
            {
                continue;
            }
            return true;
        }
    }
    false
}

pub(crate) fn strip_assignments(words: &mut Vec<Token>, path_tokens: &mut Vec<String>) {
    while words.len() > 1 && words[0].kind == TokenKind::Word && is_assignment(&words[0].raw) {
        let text = &words[0].text;
        if let Some((_, val)) = text.split_once('=')
            && is_path_token(val)
            && !path_tokens.contains(&val.to_string())
        {
            path_tokens.push(val.to_string());
        }
        words.remove(0);
    }
}

fn is_assignment(raw: &str) -> bool {
    let Some((key, _)) = raw.split_once('=') else {
        return false;
    };
    if key.is_empty() {
        return false;
    }
    let first = key.chars().next().unwrap();
    (first.is_ascii_alphabetic() || first == '_') && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn strip_wrappers(words: &mut Vec<Token>) {
    while words.len() >= 2 && words[0].kind == TokenKind::Word {
        let first = &words[0].text;
        if WRAPPERS.contains(&first.as_str()) && !words[1].raw.starts_with('-') {
            words.remove(0);
            continue;
        }
        if first == "timeout" && is_duration(&words[1].text) && words.len() > 2 {
            words.drain(0..2);
            continue;
        }
        if first == "xargs" && !words[1].raw.starts_with('-') {
            words.remove(0);
            continue;
        }
        break;
    }
}

fn is_duration(text: &str) -> bool {
    let trimmed = text.trim_end_matches(['s', 'm', 'h', 'd']);
    !trimmed.is_empty() && trimmed.parse::<f64>().is_ok()
}

pub(crate) fn collect_arg_paths(words: &[Token], path_tokens: &mut Vec<String>) {
    for word in words.iter().skip(1) {
        if word.kind == TokenKind::Word && is_path_token(&word.text) && !path_tokens.contains(&word.text) {
            path_tokens.push(word.text.clone());
        }
    }
}

fn is_path_token(token: &str) -> bool {
    if reject_non_path(token) {
        return false;
    }
    token == "~"
        || token.starts_with('/')
        || token.starts_with("~/")
        || token.starts_with('.')
        || token.contains('/')
        || token.contains("..")
}

fn reject_non_path(token: &str) -> bool {
    token.is_empty()
        || token.starts_with('-')
        || is_assignment(token)
        || is_url(token)
        || is_scoped_package(token)
        || token.chars().all(|c| c == '/')
}

fn is_url(token: &str) -> bool {
    token.contains("://")
}

fn is_scoped_package(token: &str) -> bool {
    token.starts_with('@') && !token.starts_with("@/")
}

// ---------------------------------------------------------------------------
// Multi-line formatting for compound commands
// ---------------------------------------------------------------------------

pub fn format_command_lines(command: &str) -> String {
    let token_res = tokenize(command);
    if token_res.tokens.is_empty() {
        return command.to_string();
    }
    let has_operators = token_res
        .tokens
        .iter()
        .any(|t| t.kind == TokenKind::Separator && t.raw != "\n");
    if !has_operators {
        return command.to_string();
    }
    let lines = operator_lines(&token_res.tokens);
    if lines.len() > 1 {
        indent_lines(&lines)
    } else {
        command.to_string()
    }
}

fn is_continuation_operator(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed.ends_with("&&") || trimmed.ends_with("||") || trimmed.ends_with('|') || trimmed.ends_with('&')
}

fn indent_lines(lines: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut continuation = false;
    for line in lines {
        if continuation {
            out.push(format!("  {line}"));
        } else {
            out.push(line.clone());
        }
        continuation = is_continuation_operator(line);
    }
    out.join("\n")
}

fn operator_lines(tokens: &[Token]) -> Vec<String> {
    let mut builder = LineBuilder::default();
    for token in tokens {
        match token.kind {
            TokenKind::Separator => builder.separator(&token.raw),
            _ => builder.word(&token.raw),
        }
    }
    builder.finish()
}

#[derive(Default)]
struct LineBuilder {
    lines: Vec<String>,
    current: Vec<String>,
}

impl LineBuilder {
    fn separator(&mut self, raw: &str) {
        if raw == ";" || raw == ";;" {
            self.terminate(raw);
        } else {
            self.break_line(raw);
        }
    }

    fn terminate(&mut self, terminator: &str) {
        if self.current.is_empty() {
            return;
        }
        let mut line = self.current.join(" ");
        line.push_str(terminator);
        self.lines.push(line);
        self.current.clear();
    }

    fn break_line(&mut self, raw: &str) {
        if !self.current.is_empty() {
            let mut line = self.current.join(" ");
            if !raw.trim().is_empty() {
                line.push(' ');
                line.push_str(raw);
            }
            self.lines.push(line);
            self.current.clear();
        } else if !raw.trim().is_empty() {
            self.lines.push(raw.to_string());
        }
    }

    fn word(&mut self, raw: &str) {
        self.current.push(raw.to_string());
    }

    fn finish(mut self) -> Vec<String> {
        if !self.current.is_empty() {
            self.lines.push(self.current.join(" "));
        }
        self.lines
    }
}

// ---------------------------------------------------------------------------
// Segment analysis
// ---------------------------------------------------------------------------

pub struct BashAnalysis {
    pub commands: Vec<String>,
    pub path_tokens: Vec<String>,
    pub suspicious: bool,
}

struct Segment {
    tokens: Vec<Token>,
    dangling: bool,
}

struct SegmentAnalysis {
    command: Option<String>,
    path_tokens: Vec<String>,
}

struct AnalysisCollector {
    commands: Vec<String>,
    path_tokens: Vec<String>,
    has_dangling: bool,
}

impl AnalysisCollector {
    fn new() -> Self {
        Self {
            commands: Vec::new(),
            path_tokens: Vec::new(),
            has_dangling: false,
        }
    }

    fn record_segment(&mut self, segment: Segment) {
        if segment.dangling {
            self.has_dangling = true;
        }
        let analysis = analyze_segment(segment);
        if let Some(cmd) = analysis.command {
            self.commands.push(cmd);
        }
        for token in analysis.path_tokens {
            if !self.path_tokens.contains(&token) {
                self.path_tokens.push(token);
            }
        }
    }
}

pub fn analyze_bash_command(command: &str) -> BashAnalysis {
    let token_res = tokenize(command);
    let mut collector = AnalysisCollector::new();
    for segment in split_segments(&token_res.tokens) {
        collector.record_segment(segment);
    }

    let suspicious = token_res.suspicious || collector.commands.is_empty() || collector.has_dangling;
    BashAnalysis {
        commands: collector.commands,
        path_tokens: collector.path_tokens,
        suspicious,
    }
}

fn split_segments(tokens: &[Token]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut last_sep = None;

    for token in tokens {
        if token.kind == TokenKind::Separator {
            segments.push(Segment {
                dangling: current.is_empty(),
                tokens: std::mem::take(&mut current),
            });
            last_sep = Some(token.raw.clone());
            continue;
        }
        current.push(token.clone());
    }

    let trailing_hard = check_trailing_hard(tokens, last_sep.as_deref());
    segments.push(Segment {
        dangling: trailing_hard,
        tokens: current,
    });
    segments
}

fn check_trailing_hard(tokens: &[Token], last_sep: Option<&str>) -> bool {
    let Some(last) = tokens.last() else {
        return false;
    };
    if last.kind != TokenKind::Separator {
        return false;
    }
    last_sep != Some(";") && last_sep != Some("\n")
}

fn analyze_segment(segment: Segment) -> SegmentAnalysis {
    let mut words = segment.tokens;
    let mut path_tokens = Vec::new();

    extract_redirect_targets(&words, &mut path_tokens);
    strip_assignments(&mut words, &mut path_tokens);
    strip_wrappers(&mut words);

    if words.is_empty() || words[0].kind != TokenKind::Word {
        return SegmentAnalysis {
            command: None,
            path_tokens,
        };
    }

    let command = words.iter().map(|t| t.raw.as_str()).collect::<Vec<_>>().join(" ");
    collect_arg_paths(&words, &mut path_tokens);

    SegmentAnalysis {
        command: Some(command),
        path_tokens,
    }
}
