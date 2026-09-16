mod args;
mod builder;
mod types;

pub use builder::CompletionSet;
pub use types::{BUILTIN_SLASH_COMMANDS, CommandItem, Completion, ModelItem, ProviderItem, SkillItem, THINKING_LEVELS};

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use args::complete_slash_args;

fn complete_files(files: &[String], prefix: &str, cursor: usize) -> Option<Vec<Completion>> {
    let at_idx = prefix.rfind('@')?;
    let at_is_word_start = at_idx == 0 || prefix[..at_idx].ends_with(char::is_whitespace);
    if !at_is_word_start {
        return Some(Vec::new());
    }
    let file_prefix = &prefix[at_idx + 1..];
    let lower_prefix = file_prefix.to_lowercase();
    Some(
        files
            .iter()
            .filter(|f| f.to_lowercase().contains(&lower_prefix))
            .take(25)
            .map(|f| Completion {
                value: f.clone(),
                description: None,
                replacement: at_idx..cursor,
            })
            .collect(),
    )
}

fn complete_slash_commands(commands: &[CommandItem], prefix: &str, cursor: usize) -> Vec<Completion> {
    if !prefix.starts_with('/') || prefix.contains(' ') || prefix[1..].contains('/') {
        return Vec::new();
    }
    let query = prefix.trim_start_matches('/');
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &CommandItem)> = commands
        .iter()
        .filter_map(|cmd| {
            let cmd_name = cmd.name.trim_start_matches('/');
            matcher.fuzzy_match(cmd_name, query).map(|score| (score, cmd))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    scored
        .into_iter()
        .map(|(_, cmd)| Completion {
            value: cmd.name.clone(),
            description: Some(cmd.description.clone()),
            replacement: 0..cursor,
        })
        .collect()
}

impl CompletionSet {
    pub fn complete(&self, line: &str, cursor: usize) -> Vec<Completion> {
        let Some(prefix) = line.get(..cursor) else {
            return Vec::new();
        };

        if let Some(results) = complete_slash_args(self, prefix, cursor) {
            return results;
        }
        if let Some(results) = complete_files(&self.files, prefix, cursor) {
            return results;
        }
        complete_slash_commands(&self.commands, prefix, cursor)
    }
}
