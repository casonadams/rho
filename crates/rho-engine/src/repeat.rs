use serde_json::Value;
use std::path::{Path, PathBuf};

pub const REPEATED_CALL_MESSAGE: &str = "This identical tool call was blocked after three consecutive attempts. No operation was executed. Try a semantically different approach.";

#[derive(Clone, Default)]
pub struct RepeatedCallTracker {
    working_dir: PathBuf,
    key: Option<String>,
    consecutive: usize,
}

pub type RepeatedCallHook = RepeatedCallTracker;

impl RepeatedCallTracker {
    pub fn new(working_dir: impl AsRef<Path>) -> Self {
        let p = working_dir.as_ref();
        let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        Self {
            working_dir: canonical,
            key: None,
            consecutive: 0,
        }
    }

    pub fn check(&mut self, tool_name: &str, arguments: &Value) -> Result<(), &'static str> {
        let key = normalized_call_key(tool_name, arguments, &self.working_dir);
        if self.key.as_ref() == Some(&key) {
            self.consecutive += 1;
        } else {
            self.key = Some(key);
            self.consecutive = 1;
        }
        if self.consecutive >= 3 {
            return Err(REPEATED_CALL_MESSAGE);
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.key = None;
        self.consecutive = 0;
    }
}

fn normalized_call_key(tool_name: &str, arguments: &Value, working_dir: &Path) -> String {
    let mut normalized = arguments.clone();
    match tool_name {
        "bash" => normalize_bash(&mut normalized, working_dir),
        "web_search" => normalize_web_search(&mut normalized),
        _ => {}
    }
    serde_json::to_string(&(tool_name, normalized)).unwrap_or_else(|_| format!("{tool_name}:<invalid>"))
}

fn normalize_bash(arguments: &mut Value, working_dir: &Path) {
    let Some(values) = arguments.as_object_mut() else {
        return;
    };
    if let Some(command) = values.get_mut("command")
        && let Some(text) = command.as_str()
    {
        *command = Value::String(normalize_shell_whitespace(text));
    }
    values.insert(
        "working_directory".to_string(),
        Value::String(normalize_working_dir(working_dir)),
    );
}

fn normalize_web_search(arguments: &mut Value) {
    let Some(values) = arguments.as_object_mut() else {
        return;
    };
    if let Some(query) = values.get_mut("query")
        && let Some(text) = query.as_str()
    {
        *query = Value::String(
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase(),
        );
    }
    let effective_limit = values.get("limit").and_then(Value::as_u64).unwrap_or(5).clamp(1, 20);
    values.insert("limit".to_string(), Value::from(effective_limit));
}

fn normalize_working_dir(working_dir: &Path) -> String {
    working_dir.to_string_lossy().into_owned()
}

struct ShellNormalizer {
    output: String,
    quote: Option<char>,
    escaped: bool,
    pending_space: bool,
}

impl ShellNormalizer {
    fn new() -> Self {
        Self {
            output: String::new(),
            quote: None,
            escaped: false,
            pending_space: false,
        }
    }

    fn push_non_space(&mut self, c: char) {
        if self.pending_space && !self.output.is_empty() {
            self.output.push(' ');
            self.pending_space = false;
        }
        self.output.push(c);
    }

    fn handle_quote_char(&mut self, c: char) -> bool {
        if let Some(active) = self.quote {
            self.output.push(c);
            if c == active {
                self.quote = None;
            }
            true
        } else {
            false
        }
    }

    fn handle_escaped(&mut self, c: char) -> bool {
        if self.escaped {
            self.push_non_space(c);
            self.escaped = false;
            true
        } else if c == '\\' && self.quote != Some('\'') {
            self.push_non_space(c);
            self.escaped = true;
            true
        } else {
            false
        }
    }

    fn step(&mut self, c: char) {
        if self.handle_escaped(c) || self.handle_quote_char(c) {
            return;
        }
        if matches!(c, '\'' | '"') {
            self.push_non_space(c);
            self.quote = Some(c);
        } else if c.is_whitespace() {
            self.pending_space = true;
        } else {
            self.push_non_space(c);
        }
    }
}

fn normalize_shell_whitespace(command: &str) -> String {
    let mut normalizer = ShellNormalizer::new();
    for character in command.trim().chars() {
        normalizer.step(character);
    }
    normalizer.output
}
