use std::collections::VecDeque;

use super::super::{
    BatchDecision, FlushBarrier, InteractionPrompt, InteractiveUi, OutputEvent, PendingUiBatch, UiEvent, UiPortError,
};
use crate::ui::interactive::{Activity, InteractiveState, UiAction};

#[test]
fn pending_batch_preserves_text_and_keeps_the_latest_activity() {
    let mut batch = PendingUiBatch::new(1024);
    assert!(matches!(
        batch.push(UiEvent::Output(OutputEvent::Text("one".into()))),
        BatchDecision::Pending
    ));
    batch.push(UiEvent::Activity(Activity::Thinking));
    batch.push(UiEvent::Output(OutputEvent::Text(" two".into())));
    batch.push(UiEvent::Activity(Activity::Working));

    let drained = batch.drain();
    assert_eq!(drained.text.as_bytes(), b"one two");
    assert_eq!(drained.activity, Some(Activity::Working));
    assert!(batch.is_empty());
}

#[test]
fn pending_batch_keeps_the_latest_running_tool_update() {
    let mut batch = PendingUiBatch::new(1024);
    batch.push(UiEvent::RunningTool(Some("cargo test".into())));
    batch.push(UiEvent::RunningTool(None));
    batch.push(UiEvent::RunningTool(Some("cargo build".into())));

    let drained = batch.drain();
    assert_eq!(drained.running_tool, Some(Some("cargo build".to_string())));
    assert!(batch.drain().running_tool.is_none());
}

#[test]
fn streaming_flood_preserves_output_and_applies_input_within_two_frames() {
    let fragments = (0..10_000).map(|index| format!("{index:05}|")).collect::<VecDeque<_>>();
    let expected = fragments.iter().cloned().collect::<String>();
    let mut fragments = fragments;
    let mut input = VecDeque::from([UiAction::Insert('r'), UiAction::Insert('h'), UiAction::Insert('o')]);
    let mut state = InteractiveState::default();
    let mut batch = PendingUiBatch::new(4 * 1024);
    let mut output = String::new();
    let mut frame = 0_usize;
    let mut input_visible_at = None;
    let mut fragments_since_frame = 0_usize;

    while !fragments.is_empty() || !input.is_empty() || !batch.is_empty() {
        if fragments_since_frame == 64 || fragments.is_empty() {
            output.push_str(&batch.drain().text);
            frame += 1;
            fragments_since_frame = 0;
            continue;
        }
        if let Some(action) = input.pop_front() {
            state.apply(action);
            input_visible_at.get_or_insert(frame);
            continue;
        }
        let fragment = fragments.pop_front().unwrap();
        if matches!(
            batch.push(UiEvent::Output(OutputEvent::Text(fragment))),
            BatchDecision::Flush(_)
        ) {
            output.push_str(&batch.drain().text);
        }
        fragments_since_frame += 1;
    }

    assert_eq!(state.editor().text(), "rho");
    assert!(input_visible_at.unwrap() <= 2);
    assert_eq!(output.as_bytes(), expected.as_bytes());
}

#[tokio::test]
async fn pending_batch_exposes_newline_size_and_interaction_barriers() {
    let mut newline = PendingUiBatch::new(1024);
    assert!(matches!(
        newline.push(UiEvent::Output(OutputEvent::Text("line\n".into()))),
        BatchDecision::Flush(FlushBarrier::Newline)
    ));

    let mut size = PendingUiBatch::new(4);
    assert!(matches!(
        size.push(UiEvent::Output(OutputEvent::Text("1234".into()))),
        BatchDecision::Flush(FlushBarrier::Size)
    ));

    let (ui, mut events) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Modal".into(),
            body: String::new(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
        })
        .await
    });
    let event = events.recv().await.unwrap();
    assert!(matches!(
        size.push(event),
        BatchDecision::Barrier(FlushBarrier::Interaction, UiEvent::Interaction { .. })
    ));
    drop(size);
    assert!(matches!(request.await.unwrap(), Err(UiPortError::Closed)));
}
