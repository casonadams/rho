use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use super::{
    BatchDecision, FlushBarrier, InteractionPrompt, InteractionResponse, InteractiveUi, OutputEvent, PendingUiBatch,
    UiEvent, UiPortError,
};
use crate::ui::interactive::{Activity, InteractiveState, UiAction};

#[tokio::test]
async fn channel_preserves_output_and_activity_order() {
    let (ui, mut receiver) = InteractiveUi::channel();
    ui.output(OutputEvent::Text("first".into())).unwrap();
    ui.set_activity(Activity::Thinking).unwrap();
    ui.output(OutputEvent::Text("second".into())).unwrap();

    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Output(OutputEvent::Text(text))) if text == "first"
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Activity(Activity::Thinking))
    ));
    assert!(matches!(
        receiver.recv().await,
        Some(UiEvent::Output(OutputEvent::Text(text))) if text == "second"
    ));
}

#[tokio::test]
async fn interaction_response_resolves_the_request() {
    let (ui, mut receiver) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Approval".into(),
            body: "Allow?".into(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
        })
        .await
    });
    let Some(UiEvent::Interaction { responder, .. }) = receiver.recv().await else {
        panic!("expected interaction request");
    };

    responder.respond(InteractionResponse::Selected(0)).unwrap();
    assert_eq!(request.await.unwrap().unwrap(), InteractionResponse::Selected(0));
}

#[tokio::test]
async fn dropping_responder_reports_a_closed_request() {
    let (ui, mut receiver) = InteractiveUi::channel();
    let request = tokio::spawn(async move {
        ui.request(InteractionPrompt {
            title: "Question".into(),
            body: String::new(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: true,
            initial_text: None,
        })
        .await
    });
    let Some(UiEvent::Interaction { responder, .. }) = receiver.recv().await else {
        panic!("expected interaction request");
    };
    drop(responder);

    assert!(matches!(request.await.unwrap(), Err(UiPortError::Closed)));
}

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn writer_transport_is_line_oriented_and_rejects_interactions() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let ui = InteractiveUi::writer(SharedWriter(Arc::clone(&bytes)));

    ui.output(OutputEvent::Text("plain output\n".into())).unwrap();
    ui.set_activity(Activity::Thinking).unwrap();
    let response = ui
        .request(InteractionPrompt {
            title: String::new(),
            body: String::new(),
            options: Vec::new(),
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
        })
        .await;

    assert_eq!(*bytes.lock().unwrap(), b"plain output\n");
    assert!(matches!(response, Err(UiPortError::Unavailable)));
}

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
