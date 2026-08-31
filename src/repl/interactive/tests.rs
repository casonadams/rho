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
