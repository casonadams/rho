use super::super::{InteractiveState, QueueKind, UiAction, UiEffect};

#[test]
fn submissions_keep_fifo_order_and_classification() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" steer ");
    assert_eq!(
        state.apply(UiAction::Submit(QueueKind::Steering)),
        UiEffect::Queued(super::super::QueuedMessage {
            text: "steer".to_string(),
            kind: QueueKind::Steering,
        })
    );
    state.editor_mut().set_text("follow");
    state.apply(UiAction::Submit(QueueKind::FollowUp));

    assert_eq!(state.queue_len(), 2);
    assert_eq!(state.pop_queued().unwrap().kind, QueueKind::Steering);
    assert_eq!(state.pop_queued().unwrap().kind, QueueKind::FollowUp);
}

#[test]
fn dequeue_all_extracts_all_queued_messages() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("first");
    state.apply(UiAction::Submit(QueueKind::Steering));
    state.editor_mut().set_text("second");
    state.apply(UiAction::Submit(QueueKind::FollowUp));

    assert_eq!(state.queue_len(), 2);
    let dequeued = state.dequeue_all();
    assert_eq!(dequeued.len(), 2);
    assert_eq!(dequeued[0].text, "first");
    assert_eq!(dequeued[1].text, "second");
    assert_eq!(state.queue_len(), 0);
}

#[test]
fn empty_submissions_are_ignored() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text(" \n ");
    assert_eq!(state.apply(UiAction::Submit(QueueKind::Steering)), UiEffect::None);
    assert_eq!(state.queue_len(), 0);
}
