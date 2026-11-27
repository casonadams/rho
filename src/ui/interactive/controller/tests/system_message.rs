use super::fake::controller;

#[test]
fn system_message_sets_and_clears_on_controller() {
    let (mut controller, _, _) = controller(80);

    assert_eq!(controller.state().system_message(), None);

    controller.set_system_message("Model: claude-3-5-sonnet");
    assert_eq!(controller.state().system_message(), Some("Model: claude-3-5-sonnet"));

    controller.clear_system_message();
    assert_eq!(controller.state().system_message(), None);
}

#[test]
fn system_message_check_expiration_returns_false_when_not_expired() {
    let (mut controller, _, _) = controller(80);

    controller.set_system_message("Tool output: expanded");
    assert!(!controller.check_system_message_expiration());
    assert_eq!(controller.state().system_message(), Some("Tool output: expanded"));
}
