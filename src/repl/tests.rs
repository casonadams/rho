use super::completer::RhoCompleter;
use super::submitted_input_rows;
use reedline::Completer;

#[test]
fn slash_commands_complete_from_a_prefix() {
    let mut completer = RhoCompleter::new(&[], Vec::new(), vec!["review".to_string()]);
    let suggestions = completer.complete("/mo", 3);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].value, "/model");

    let tmpl_suggestions = completer.complete("/rev", 4);
    assert_eq!(tmpl_suggestions.len(), 1);
    assert_eq!(tmpl_suggestions[0].value, "/review");
}

#[test]
fn skill_names_complete_from_prefix() {
    let skill_names = crate::skills::resolved_skills(None, None)
        .into_iter()
        .map(|skill| skill.metadata.name)
        .collect();
    let mut completer = RhoCompleter::new(&[], skill_names, Vec::new());
    let suggestions = completer.complete("/skill pl", 9);
    assert!(suggestions.iter().any(|s| s.value == "/skill plan"));
}

#[test]
fn submitted_input_rows_include_prompt_width_and_terminal_wrapping() {
    assert_eq!(submitted_input_rows("hello", 80), 1);
    assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
    assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
    assert_eq!(submitted_input_rows("界界", 5), 2);
}
