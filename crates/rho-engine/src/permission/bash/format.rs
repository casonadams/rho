use super::lexer::tokenize;
use super::token::{Token, TokenKind};

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

fn indent_lines(lines: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut continuation = false;
    for line in lines {
        if continuation {
            out.push(format!("  {line}"));
        } else {
            out.push(line.clone());
        }
        continuation = !line.ends_with(';');
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
            self.lines.push(self.current.join(" "));
            self.current.clear();
        }
        if !raw.trim().is_empty() {
            self.current.push(raw.to_string());
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
