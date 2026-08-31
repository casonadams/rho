use std::{ops::Range, path::PathBuf};

use reedline::{FileBackedHistory, History, HistoryItem, SearchDirection, SearchQuery};

use super::commands::SLASH_COMMANDS;

pub const CURATED_MODELS: &[(&str, &str)] = &[
    ("gemini-3.7-flash", "antigravity"),
    ("gpt-5.6-luna", "chatgpt"),
    ("gpt-5.4", "chatgpt"),
    ("claude-sonnet-4-6", "anthropic"),
    ("gpt-4o", "openai"),
    ("gemini-2.5-pro", "gemini"),
    ("deepseek-reasoner", "deepseek"),
];
pub const PROVIDERS: &[&str] = &[
    "antigravity",
    "anthropic",
    "openai",
    "chatgpt",
    "copilot",
    "gemini",
    "deepseek",
    "groq",
    "openrouter",
    "ollama",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub value: String,
    pub replacement: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct CompletionSet {
    commands: Vec<String>,
    skills: Vec<String>,
    models: Vec<String>,
    providers: Vec<String>,
    files: Vec<String>,
}

struct MatchRequest<'a> {
    prefix: &'a str,
    leader: &'a str,
    cursor: usize,
}

impl CompletionSet {
    pub fn rho(extension_commands: &[(&str, &str)], skill_names: Vec<String>, prompt_templates: Vec<String>) -> Self {
        let mut commands = SLASH_COMMANDS
            .iter()
            .map(|command| (*command).to_string())
            .collect::<Vec<_>>();
        commands.extend(extension_commands.iter().map(|(name, _)| format!("/{name}")));
        commands.extend(prompt_templates.iter().map(|name| format!("/{name}")));
        commands.sort();
        commands.dedup();

        let cwd = std::env::current_dir().ok();
        let files = cwd
            .as_deref()
            .map(|d| rho_core::workspace::list_relative_files(d, 2000))
            .unwrap_or_default();

        Self {
            commands,
            skills: skill_names,
            models: CURATED_MODELS.iter().map(|(model, _)| (*model).to_string()).collect(),
            providers: PROVIDERS.iter().map(|provider| (*provider).to_string()).collect(),
            files,
        }
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files = files;
        self
    }

    pub fn complete(&self, line: &str, cursor: usize) -> Vec<Completion> {
        let Some(prefix) = line.get(..cursor) else {
            return Vec::new();
        };
        if let Some(argument) = prefix
            .strip_prefix("/skill ")
            .or_else(|| prefix.strip_prefix("/skills "))
        {
            matches(
                &self.skills,
                MatchRequest {
                    prefix: argument,
                    leader: "/skill ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/model ") {
            matches(
                &self.models,
                MatchRequest {
                    prefix: argument,
                    leader: "/model ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/login ") {
            matches(
                &self.providers,
                MatchRequest {
                    prefix: argument,
                    leader: "/login ",
                    cursor,
                },
            )
        } else if let Some(argument) = prefix.strip_prefix("/logout ") {
            matches(
                &self.providers,
                MatchRequest {
                    prefix: argument,
                    leader: "/logout ",
                    cursor,
                },
            )
        } else if let Some(at_idx) = prefix.rfind('@') {
            let at_is_word_start = at_idx == 0 || prefix[..at_idx].ends_with(char::is_whitespace);
            if at_is_word_start {
                let file_prefix = &prefix[at_idx + 1..];
                self.files
                    .iter()
                    .filter(|f| f.to_lowercase().contains(&file_prefix.to_lowercase()))
                    .take(25)
                    .map(|f| Completion {
                        value: f.clone(),
                        replacement: at_idx..cursor,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else if prefix.starts_with('/') {
            matches(
                &self.commands,
                MatchRequest {
                    prefix,
                    leader: "",
                    cursor,
                },
            )
        } else {
            Vec::new()
        }
    }
}

fn matches(values: &[String], request: MatchRequest<'_>) -> Vec<Completion> {
    values
        .iter()
        .filter(|value| value.starts_with(request.prefix))
        .map(|value| Completion {
            value: format!("{}{value}", request.leader),
            replacement: 0..request.cursor,
        })
        .collect()
}

pub struct InteractiveHistory {
    storage: FileBackedHistory,
    entries: Vec<String>,
    capacity: usize,
    position: Option<usize>,
    saved_draft: Option<String>,
}

impl InteractiveHistory {
    pub fn with_file(capacity: usize, path: PathBuf) -> reedline::Result<Self> {
        let storage = FileBackedHistory::with_file(capacity, path)?;
        let entries = storage
            .search(SearchQuery::everything(SearchDirection::Forward, None))?
            .into_iter()
            .map(|item| item.command_line)
            .collect();
        Ok(Self {
            storage,
            entries,
            capacity,
            position: None,
            saved_draft: None,
        })
    }

    pub fn record(&mut self, value: &str) -> reedline::Result<()> {
        self.reset_navigation();
        if value.is_empty() || self.capacity == 0 || self.entries.last().is_some_and(|entry| entry == value) {
            return Ok(());
        }
        self.storage.save(HistoryItem::from_command_line(value))?;
        if self.entries.len() == self.capacity {
            self.entries.remove(0);
        }
        self.entries.push(value.to_string());
        Ok(())
    }

    pub fn previous(&mut self, current_draft: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let position = match self.position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.saved_draft = Some(current_draft.to_string());
                self.entries.len() - 1
            }
        };
        self.position = Some(position);
        self.entries.get(position).cloned()
    }

    pub fn next_entry(&mut self) -> Option<String> {
        let position = self.position?;
        if position + 1 < self.entries.len() {
            self.position = Some(position + 1);
            return self.entries.get(position + 1).cloned();
        }
        self.position = None;
        self.saved_draft.take()
    }

    pub fn reset_navigation(&mut self) {
        self.position = None;
        self.saved_draft = None;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CompletionSet, InteractiveHistory};

    #[test]
    fn completion_reports_replacement_spans_for_commands_and_arguments() {
        let completions = CompletionSet::rho(&[("deploy", "Deploy")], Vec::new(), Vec::new());

        let command = completions.complete("/dep trailing", 4);
        assert_eq!(command[0].value, "/deploy");
        assert_eq!(command[0].replacement, 0..4);

        let model = completions.complete("/model gpt-5.4 suffix", 14);
        assert_eq!(model[0].value, "/model gpt-5.4");
        assert_eq!(model[0].replacement, 0..14);
    }

    #[test]
    fn completion_rejects_invalid_cursor_boundaries() {
        let completions = CompletionSet::rho(&[], Vec::new(), Vec::new());
        assert!(completions.complete("/model 界", 8).is_empty());
        assert!(completions.complete("/model", 99).is_empty());
    }

    #[test]
    fn history_navigation_restores_the_active_draft() {
        let path = std::env::temp_dir().join(format!("rho-history-{}.txt", uuid::Uuid::new_v4()));
        let mut history = InteractiveHistory::with_file(3, path.clone()).unwrap();
        history.record("first").unwrap();
        history.record("second").unwrap();

        assert_eq!(history.previous("draft").as_deref(), Some("second"));
        assert_eq!(history.previous("ignored").as_deref(), Some("first"));
        assert_eq!(history.previous("ignored").as_deref(), Some("first"));
        assert_eq!(history.next_entry().as_deref(), Some("second"));
        assert_eq!(history.next_entry().as_deref(), Some("draft"));
        drop(history);

        let mut reopened = InteractiveHistory::with_file(3, path.clone()).unwrap();
        assert_eq!(reopened.previous("").as_deref(), Some("second"));
        drop(reopened);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_ignores_empty_duplicate_entries_and_enforces_capacity() {
        let path = std::env::temp_dir().join(format!("rho-history-{}.txt", uuid::Uuid::new_v4()));
        let mut history = InteractiveHistory::with_file(2, path.clone()).unwrap();
        history.record("").unwrap();
        history.record("one").unwrap();
        history.record("one").unwrap();
        history.record("two\nlines").unwrap();
        history.record("three").unwrap();
        drop(history);

        let mut reopened = InteractiveHistory::with_file(2, path.clone()).unwrap();
        assert_eq!(reopened.previous("").as_deref(), Some("three"));
        assert_eq!(reopened.previous("").as_deref(), Some("two\nlines"));
        assert_eq!(reopened.previous("").as_deref(), Some("two\nlines"));
        drop(reopened);
        fs::remove_file(path).unwrap();
    }
}
