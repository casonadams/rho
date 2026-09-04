mod args;
mod builder;
mod types;

pub use builder::CompletionSet;
pub use types::{BUILTIN_SLASH_COMMANDS, CommandItem, Completion, ModelItem, ProviderItem, SkillItem, THINKING_LEVELS};

use crate::repl::interactive::fuzzy::fuzzy_match;
use args::complete_slash_args;

impl CompletionSet {
    pub fn complete(&self, line: &str, cursor: usize) -> Vec<Completion> {
        let Some(prefix) = line.get(..cursor) else {
            return Vec::new();
        };

        if let Some(results) = complete_slash_args(self, prefix, cursor) {
            results
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
                        description: None,
                        replacement: at_idx..cursor,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else if prefix.starts_with('/') && !prefix.contains(' ') {
            let query = prefix.trim_start_matches('/');
            let mut scored: Vec<(i32, &CommandItem)> = self
                .commands
                .iter()
                .filter_map(|cmd| {
                    let cmd_name = cmd.name.trim_start_matches('/');
                    fuzzy_match(query, cmd_name).map(|score| (score, cmd))
                })
                .collect();

            scored.sort_by_key(|(score, cmd)| (*score, cmd.name.clone()));

            scored
                .into_iter()
                .map(|(_, cmd)| Completion {
                    value: cmd.name.clone(),
                    description: Some(cmd.description.clone()),
                    replacement: 0..cursor,
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}
