use super::fake::{FakeTerminal, Operation, controller};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{Activity, InteractiveState};

#[test]
fn busy_working_line_renders_above_the_editor() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Thinking;
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    let working_index = ops.iter().position(|op| {
        matches!(
            op,
            Operation::Write(text) if text.contains("thinking") || text.contains("working")
        )
    });
    let divider_index = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text.contains(&"\u{2500}".repeat(60))));

    assert!(working_index.is_some());
    assert!(working_index.unwrap() < divider_index.unwrap());
}

#[test]
fn busy_working_line_disappears_when_idle() {
    let (backend, operations, _) = FakeTerminal::new(20);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    let mut controller = TerminalController::new(backend, state).unwrap();
    controller.state_mut().footer_mut().activity = Activity::Idle;
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("working") || text.contains("thinking")))
    );
}

#[test]
fn footer_carries_no_spinner_or_activity_label_when_busy() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    state.footer_mut().model = "model".into();
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("model") && text.contains("\u{1b}[2m")))
    );
    let rendered_footer = &controller.rendered.as_ref().unwrap().footer;
    assert!(!rendered_footer.contains("working"));
    assert!(!rendered_footer.contains("thinking"));
}

#[test]
fn unfocused_controller_preserves_working_line_and_suppresses_software_cursor() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    let mut controller = TerminalController::new(backend, state).unwrap();
    controller.set_focused(false);
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("working"))),
        "working line with spinner text must remain visible when unfocused"
    );
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("\x1b[7m"))),
        "software cursor must be suppressed when unfocused"
    );
}

#[test]
fn idle_footer_is_rendered_dimmed() {
    let (mut controller, operations, _) = controller(20);
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    assert!(
        operations
            .borrow()
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("\u{1b}[2m")))
    );
}

#[test]
fn modal_suppresses_persistent_a2ui_widget_lines() {
    use crate::ui::interactive::ModalState;
    use rho_harness_core::a2ui::{
        A2uiComponent, A2uiComponentKind, A2uiMessage, BoundString, CreateSurface, SurfacePlacement, TextUsageHint,
        UpdateComponents,
    };

    let (backend, _, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.set_a2ui_widgets(true);
    for (id, placement) in [
        ("above:1", SurfacePlacement::AboveEditor),
        ("below:1", SurfacePlacement::BelowEditor),
    ] {
        state
            .apply_a2ui_message(A2uiMessage::CreateSurface(CreateSurface {
                surface_id: id.to_string(),
                catalog_id: None,
                placement,
                title: None,
                priority: 10,
                root_id: "root".to_string(),
            }))
            .unwrap();
        state
            .apply_a2ui_message(A2uiMessage::UpdateComponents(UpdateComponents {
                surface_id: id.to_string(),
                components: vec![A2uiComponent::new(
                    "root",
                    A2uiComponentKind::Text {
                        text: BoundString::literal("persistent widget"),
                        usage_hint: TextUsageHint::Body,
                        tone: None,
                    },
                )],
            }))
            .unwrap();
    }

    let mut controller = TerminalController::new(backend, state).unwrap();
    assert!(!controller.active_widget_lines().is_empty());
    assert!(!controller.below_widget_lines().is_empty());

    controller.state_mut().push_modal(ModalState::new("Test", "", vec![]));
    assert!(controller.active_widget_lines().is_empty());
    assert!(controller.below_widget_lines().is_empty());
}
