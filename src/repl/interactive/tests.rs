use std::fs;

use super::{CompletionSet, InteractiveHistory, ModelItem};

#[test]
fn completion_reports_replacement_spans_for_commands_and_arguments() {
    let sources = super::CompletionSources::new()
        .with_templates(vec!["deploy".to_string()])
        .with_models(vec![ModelItem {
            id: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            description: "128k ctx".to_string(),
        }]);
    let completions = CompletionSet::from_sources(sources);

    let command = completions.complete("/dep", 4);
    assert_eq!(command[0].value, "/deploy");
    assert_eq!(command[0].replacement, 0..4);

    let model = completions.complete("/model gpt-4o suffix", 13);
    assert_eq!(model[0].value, "/model gpt-4o");
    assert_eq!(model[0].replacement, 0..13);
}

#[test]
fn completion_rejects_invalid_cursor_boundaries() {
    let completions = CompletionSet::from_sources(super::CompletionSources::default());
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
    assert_eq!(history.next_entry(), None);

    let _ = fs::remove_file(path);
}
