use crate::repl::interactive::{CompletionSet, InteractiveHistory};

/// Predicts the inline completion suffix based on recent history or command completion.
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

    // 1. If starting with '/', predict slash commands or their argument templates
    if draft.starts_with('/') {
        let cmd_pred = predict_slash_command(draft, completions);
        if cmd_pred.is_some() {
            return cmd_pred;
        }
    }

    // 2. Search history in reverse for a prompt starting with `draft`
    predict_history_suffix(draft, history)
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
            return Some(entry[draft.len()..].to_string());
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

        let temp_dir = std::env::temp_dir().join(format!("rho-hist-{}", uuid::Uuid::new_v4()));
        let history = InteractiveHistory::with_file(10, temp_dir).unwrap();

        assert_eq!(
            predict_inline_completion("/mod", 4, &history, &completions),
            Some("el".to_string())
        );
        assert_eq!(predict_inline_completion("/model", 6, &history, &completions), None);
    }

    #[test]
    fn test_predict_history() {
        let completions = CompletionSet::from_sources(CompletionSources::new());
        let temp_dir = std::env::temp_dir().join(format!("rho-hist-{}", uuid::Uuid::new_v4()));
        let mut history = InteractiveHistory::with_file(10, temp_dir).unwrap();
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
}
