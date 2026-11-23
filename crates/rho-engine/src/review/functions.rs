//! Finds function definitions in a prepared source: name, parameters, and extent.

use super::lang::Lang;
use super::source::Source;
use regex::Regex;
use std::sync::LazyLock;

const MAX_HEADER_LINES: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    /// Indexes into `Source::lines`; `end` is inclusive.
    pub start: usize,
    pub end: usize,
    pub params: Vec<String>,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid function pattern")
}

static RUST: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe)\s+)*fn\s+(\w+)"));
static JS_FN: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(?:export\s+)?(?:default\s+)?(?:async\s+)?function\b\s*\*?\s*(\w*)"));
static JS_ARROW: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:export\s+)?(?:const|let)\s+(\w+)\s*=\s*(?:async\s*)?"));
static PYTHON: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:async\s+)?def\s+(\w+)"));
static GO: LazyLock<Regex> = LazyLock::new(|| re(r"^func\s+(?:\([^)]*\)\s*)?(\w+)"));

fn signature(lang: Lang, code: &str) -> Option<(String, usize)> {
    let patterns: &[&Regex] = match lang {
        Lang::Rust => &[&RUST],
        Lang::Js => &[&JS_FN, &JS_ARROW],
        Lang::Python => &[&PYTHON],
        Lang::Go => &[&GO],
        _ => &[],
    };
    patterns.iter().find_map(|pattern| {
        let caps = pattern.captures(code)?;
        Some((caps[1].to_string(), caps.get(0)?.end()))
    })
}

/// The text inside the bracket pair that opens at the start of `text`, and the index after it.
fn balanced(text: &str, open: char, close: char) -> Option<(&str, usize)> {
    if !text.starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut prev = ' ';
    for (i, c) in text.char_indices() {
        let arrow = prev == '-' && c == '>';
        prev = c;
        if c == open {
            depth += 1;
        } else if c == close && !arrow {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some((&text[open.len_utf8()..i], i + 1));
            }
        }
    }
    None
}

fn params_inside(header: &str, after_name: usize) -> Option<&str> {
    let mut rest = header[after_name..].trim_start();
    for (open, close) in [('<', '>'), ('[', ']')] {
        if rest.starts_with(open) {
            rest = rest[balanced(rest, open, close)?.1..].trim_start();
        }
    }
    balanced(rest, '(', ')').map(|(inside, _)| inside)
}

fn split_params(inside: &str) -> Vec<String> {
    let mut params = Vec::new();
    let (mut depth, mut start, mut prev) = (0i32, 0, ' ');
    for (i, c) in inside.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '>' if prev != '-' && prev != '=' => depth -= 1,
            ',' if depth <= 0 => {
                params.push(inside[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
        prev = c;
    }
    params.push(inside[start..].trim().to_string());
    params
        .into_iter()
        .filter(|p| !p.is_empty() && !is_receiver(p))
        .collect()
}

fn is_receiver(param: &str) -> bool {
    matches!(param, "self" | "&self" | "&mut self" | "mut self" | "cls") || param.starts_with("self:")
}

/// The index of the line that ends the signature starting at `start`, and its text joined.
fn header(source: &Source, start: usize) -> Option<(usize, String)> {
    let python = source.lang == Lang::Python;
    let mut text = String::new();
    for (i, line) in source.lines.iter().enumerate().skip(start).take(MAX_HEADER_LINES) {
        text.push_str(&line.code);
        text.push(' ');
        let ends = if python {
            line.code.ends_with(':')
        } else {
            line.code.contains('{')
        };
        if ends {
            return Some((i, text));
        }
        if !python && line.code.ends_with(';') {
            return None;
        }
    }
    None
}

fn brace_end(source: &Source, start: usize) -> usize {
    let mut depth = 0i32;
    let mut opened = false;
    for (i, line) in source.lines.iter().enumerate().skip(start) {
        if line.comment {
            continue;
        }
        for c in line.code.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return i;
        }
    }
    source.lines.len() - 1
}

fn indent_end(source: &Source, header_end: usize) -> usize {
    let indent = source.lines[header_end].indent;
    let mut end = header_end;
    for (i, line) in source.lines.iter().enumerate().skip(header_end + 1) {
        if line.raw.is_empty() {
            continue;
        }
        if line.indent <= indent {
            break;
        }
        end = i;
    }
    end
}

#[must_use]
pub fn find(source: &Source) -> Vec<Function> {
    let mut found = Vec::new();
    for (start, line) in source.lines.iter().enumerate() {
        if line.comment {
            continue;
        }
        let Some((name, after)) = signature(source.lang, &line.code) else {
            continue;
        };
        let Some((header_end, text)) = header(source, start) else {
            continue;
        };
        let end = if source.lang == Lang::Python {
            indent_end(source, header_end)
        } else {
            brace_end(source, start)
        };
        let arrow_ok = source.lang != Lang::Js || line.code.contains("function") || text.contains("=>");
        let params = params_inside(&text, after).filter(|_| arrow_ok);
        if let Some(inside) = params {
            found.push(Function {
                name,
                start,
                end,
                params: split_params(inside),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funcs(path: &str, text: &str) -> Vec<Function> {
        find(&Source::from_text(path, text))
    }

    #[test]
    fn rust_functions_have_extent_and_params_without_receivers() {
        let f = funcs(
            "a.rs",
            "impl X {\n    pub async fn go<T: Fn(u8) -> u8>(\n        &self,\n        a: HashMap<String, u8>,\n        b: bool,\n    ) -> u8 {\n        if a {\n        }\n    }\n    fn decl(&self);\n}\n",
        );
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].name.as_str(), f[0].start, f[0].end), ("go", 1, 8));
        assert_eq!(f[0].params, ["a: HashMap<String, u8>", "b: bool"]);
    }

    #[test]
    fn python_functions_end_at_the_dedent() {
        let f = funcs(
            "a.py",
            "def a(x, y=1):\n    z = 1\n\n    return z\n\ndef b():\n    pass\n",
        );
        assert_eq!((f[0].start, f[0].end, f[0].params.len()), (0, 3, 2));
        assert_eq!(f[1].name, "b");
    }

    #[test]
    fn js_functions_and_block_arrows_are_found_but_expression_arrows_are_not() {
        let f = funcs(
            "a.ts",
            "export async function a(x: number, y: string) {\n  return 1;\n}\nconst b = (x) => {\n  return x;\n};\nconst c = (x) => x + 1;\nconst d = (1 + 2);\n",
        );
        let names: Vec<&str> = f.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(f[1].end, 5);
    }

    #[test]
    fn go_methods_skip_the_receiver() {
        let f = funcs("a.go", "func (s *S) Run(a int, b bool) error {\n\treturn nil\n}\n");
        assert_eq!((f[0].name.as_str(), f[0].params.len(), f[0].end), ("Run", 2, 2));
    }

    #[test]
    fn braces_inside_strings_and_comments_do_not_move_the_end() {
        let f = funcs("a.rs", "fn a() {\n    let s = \"}\";\n    // }\n}\nfn b() {}\n");
        assert_eq!((f[0].end, f[1].start), (3, 4));
    }
}
