use crate::repl::interactive::{CompletionSet, InteractiveHistory, WordCompleter};

/// Predicts the inline completion suffix based on command completion, recent history, or local word completion.
pub fn predict_inline_completion(
    draft: &str,
    cursor: usize,
    history: &InteractiveHistory,
    completions: &CompletionSet,
) -> Option<String> {
    // Only predict when typing at the end of the input and input is not empty
    if draft.is_empty() || cursor != draft.len() {
        return None;
    }

    // Tier A: If starting with '/', predict slash commands
    if draft.starts_with('/')
        && let Some(cmd_pred) = predict_slash_command(draft, completions)
    {
        return Some(cmd_pred);
    }

    // Tier B: Search history in reverse for a prompt starting with `draft`
    if let Some(history_pred) = predict_history_suffix(draft, history) {
        return Some(history_pred);
    }

    // Tier C: Instant trailing-word completion
    predict_word_suffix(draft, history)
}

fn predict_word_suffix(draft: &str, history: &InteractiveHistory) -> Option<String> {
    WordCompleter::complete_draft(draft, history)
}

fn predict_slash_command(draft: &str, completions: &CompletionSet) -> Option<String> {
    let candidate = completions
        .commands
        .iter()
        .find(|c| c.name.starts_with(draft) && c.name != draft)?;
    Some(candidate.name[draft.len()..].to_string())
}

fn predict_history_suffix(draft: &str, history: &InteractiveHistory) -> Option<String> {
    for entry in history.entries().iter().rev() {
        if entry.starts_with(draft) && entry.len() > draft.len() {
            let suffix = &entry[draft.len()..];
            let first_line = suffix.split(['\n', '\r']).next().unwrap_or("");
            if !first_line.is_empty() {
                return Some(first_line.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repl::interactive::{CommandItem, CompletionSources};

    #[test]
    fn test_predict_slash_command() {
        let completions = CompletionSet::from_sources(CompletionSources::new()).with_commands(vec![CommandItem {
            name: "/model".to_string(),
            description: "Switch model".to_string(),
        }]);

        let temp_dir = rho_test_support::TestDir::new();
        let history = InteractiveHistory::with_file(10, temp_dir.join("history.txt")).unwrap();

        assert_eq!(
            predict_inline_completion("/mod", 4, &history, &completions),
            Some("el".to_string())
        );
        assert_eq!(predict_inline_completion("/model", 6, &history, &completions), None);
    }

    #[test]
    fn test_predict_history() {
        let completions = CompletionSet::from_sources(CompletionSources::new());
        let temp_dir = rho_test_support::TestDir::new();
        let mut history = InteractiveHistory::with_file(10, temp_dir.join("history.txt")).unwrap();
        history.record("git status").unwrap();
        history.record("git commit -m 'feat: test'").unwrap();

        assert_eq!(
            predict_inline_completion("git com", 7, &history, &completions),
            Some("mit -m 'feat: test'".to_string())
        );
        assert_eq!(
            predict_inline_completion("git stat", 8, &history, &completions),
            Some("us".to_string())
        );
        // Not at cursor end -> no prediction
        assert_eq!(predict_inline_completion("git com", 3, &history, &completions), None);
    }

    #[test]
    fn test_predict_history_strips_multiline_suffixes() {
        let completions = CompletionSet::from_sources(CompletionSources::new());
        let temp_dir = rho_test_support::TestDir::new();
        let mut history = InteractiveHistory::with_file(10, temp_dir.join("history.txt")).unwrap();
        history.record("git commit\n-m 'body'").unwrap();

        let suffix = predict_inline_completion("git com", 7, &history, &completions);
        assert_eq!(suffix, Some("mit".to_string()));
        assert!(
            suffix
                .as_deref()
                .is_some_and(|s| !s.contains('\n') && !s.contains('\r'))
        );

        // Draft already ends at the newline boundary -> nothing on this line to predict
        assert_eq!(
            predict_inline_completion("git commit", 10, &history, &completions),
            None
        );
    }

    #[test]
    fn test_predict_word_completion_tiers_and_guards() {
        let completions = CompletionSet::from_sources(CompletionSources::new());
        let temp_dir = rho_test_support::TestDir::new();
        let mut history = InteractiveHistory::with_file(10, temp_dir.join("history.txt")).unwrap();
        history.record("check ripwire").unwrap();

        // Common word completion
        assert_eq!(
            predict_inline_completion("that's awe", 10, &history, &completions),
            Some("some".to_string())
        );
        // ALL-CAPS preservation
        assert_eq!(
            predict_inline_completion("AWE", 3, &history, &completions),
            Some("SOME".to_string())
        );
        // History word beats common/dict word
        assert_eq!(
            predict_inline_completion("run rip", 7, &history, &completions),
            Some("wire".to_string())
        );
        // Paths, @mentions, slash commands, short tokens, non-end cursor do not trigger
        assert_eq!(predict_inline_completion("src/awe", 7, &history, &completions), None);
        assert_eq!(predict_inline_completion("@awe", 4, &history, &completions), None);
        assert_eq!(predict_inline_completion("/aw", 3, &history, &completions), None);
        assert_eq!(predict_inline_completion("aw", 2, &history, &completions), None);
        assert_eq!(predict_inline_completion("that's awe", 5, &history, &completions), None);
    }
}
