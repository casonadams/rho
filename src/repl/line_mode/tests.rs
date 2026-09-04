use super::shell::{ShellAction, handle_shell_command, submitted_input_rows};
use crate::ui::TerminalRenderer;

#[test]
fn submitted_input_rows_calculates_wrapped_height() {
    assert_eq!(submitted_input_rows("hello", 80), 1);
    assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
    assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
    assert_eq!(submitted_input_rows("界界", 5), 2);
}

#[tokio::test]
async fn plain_input_is_passthrough() {
    let renderer = TerminalRenderer::default();
    let action = handle_shell_command("plain prompt", &renderer).await;
    match action {
        ShellAction::Passthrough => {}
        _ => panic!("Expected ShellAction::Passthrough"),
    }
}
