use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::{ActivePromptRunner, ActiveQueueResult, CoordinatorInput, run_active_queue};
use crate::ui::interactive::{QueueKind, QueuedMessage};

pub(super) type PermitSender = mpsc::UnboundedSender<Result<(), &'static str>>;
pub(super) type StartedReceiver = mpsc::UnboundedReceiver<String>;
pub(super) type Timeline = Arc<Mutex<Vec<String>>>;

pub(super) struct FakeRunner {
    pub(super) permits: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<(), &'static str>>>,
    pub(super) started: mpsc::UnboundedSender<String>,
    pub(super) timeline: Timeline,
}

#[async_trait::async_trait]
impl ActivePromptRunner for FakeRunner {
    type Error = &'static str;

    async fn run_prompt(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.timeline.lock().unwrap().push(format!("started:{}", prompt.text));
        self.started.send(prompt.text.clone()).unwrap();
        let result = {
            let mut guard = self.permits.lock().await;
            guard.recv().await.unwrap()
        };
        self.timeline.lock().unwrap().push(format!("finished:{}", prompt.text));
        result
    }

    async fn steer(&self, prompt: &QueuedMessage) -> Result<(), Self::Error> {
        self.timeline.lock().unwrap().push(format!("steered:{}", prompt.text));
        Ok(())
    }

    async fn cancel_active(&self) -> Result<(), Self::Error> {
        self.timeline.lock().unwrap().push("cancelled".to_string());
        Ok(())
    }
}

pub(super) fn prompt(text: &str, kind: QueueKind) -> QueuedMessage {
    QueuedMessage {
        text: text.to_string(),
        kind,
    }
}

pub(super) fn fake_runner() -> (FakeRunner, PermitSender, StartedReceiver, Timeline) {
    let (permit_sender, permits) = mpsc::unbounded_channel();
    let (started, started_receiver) = mpsc::unbounded_channel();
    let timeline = Arc::new(Mutex::new(Vec::new()));
    (
        FakeRunner {
            permits: tokio::sync::Mutex::new(permits),
            started,
            timeline: Arc::clone(&timeline),
        },
        permit_sender,
        started_receiver,
        timeline,
    )
}

mod cancellation;
mod queue;
mod steering;
