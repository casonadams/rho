use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use super::super::{InteractionPrompt, InteractionResponse, InteractiveUi, OutputEvent, UiEvent, UiPortError};
use crate::ui::interactive::Activity;

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
