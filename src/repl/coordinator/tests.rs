use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::{ActivePromptRunner, ActiveQueueResult, CoordinatorInput, run_active_queue};
use crate::ui::interactive::{QueueKind, QueuedMessage};

type PermitSender = mpsc::UnboundedSender<Result<(), &'static str>>;
type StartedReceiver = mpsc::UnboundedReceiver<String>;
type Timeline = Arc<Mutex<Vec<String>>>;

struct FakeRunner {
    permits: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<(), &'static str>>>,
    started: mpsc::UnboundedSender<String>,
    timeline: Timeline,
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

fn prompt(text: &str, kind: QueueKind) -> QueuedMessage {
    QueuedMessage {
        text: text.to_string(),
        kind,
    }
}

fn fake_runner() -> (FakeRunner, PermitSender, StartedReceiver, Timeline) {
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

#[tokio::test]
async fn steering_prompts_are_delivered_mid_run_and_follow_ups_run_after() {
    let (runner, permits, mut started, timeline) = fake_runner();
    let (input_sender, mut input) = mpsc::unbounded_channel();
    let runner_ref = Arc::new(runner);
    let runner_clone = Arc::clone(&runner_ref);
    let task = tokio::spawn(async move {
        run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
    });

    assert_eq!(started.recv().await.as_deref(), Some("active"));
    input_sender
        .send(CoordinatorInput::Prompt(prompt("steer", QueueKind::Steering)))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    input_sender
        .send(CoordinatorInput::Prompt(prompt("follow", QueueKind::FollowUp)))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    permits.send(Ok(())).unwrap();
    assert_eq!(started.recv().await.as_deref(), Some("follow"));
    permits.send(Ok(())).unwrap();

    let ActiveQueueResult::Completed { delivered, .. } = task.await.unwrap() else {
        panic!("queue should complete");
    };
    assert_eq!(
        delivered,
        [
            prompt("steer", QueueKind::Steering),
            prompt("active", QueueKind::Steering),
            prompt("follow", QueueKind::FollowUp),
        ]
    );
    assert_eq!(
        *timeline.lock().unwrap(),
        [
            "started:active",
            "steered:steer",
            "finished:active",
            "started:follow",
            "finished:follow",
        ]
    );
}

#[tokio::test]
async fn multiple_follow_ups_run_fifo_after_active_run() {
    let (runner, permits, mut started, timeline) = fake_runner();
    let (input_sender, mut input) = mpsc::unbounded_channel();
    let runner_ref = Arc::new(runner);
    let runner_clone = Arc::clone(&runner_ref);
    let task = tokio::spawn(async move {
        run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
    });

    assert_eq!(started.recv().await.as_deref(), Some("active"));
    input_sender
        .send(CoordinatorInput::Prompt(prompt("follow1", QueueKind::FollowUp)))
        .unwrap();
    input_sender
        .send(CoordinatorInput::Prompt(prompt("follow2", QueueKind::FollowUp)))
        .unwrap();
    permits.send(Ok(())).unwrap();
    assert_eq!(started.recv().await.as_deref(), Some("follow1"));
    permits.send(Ok(())).unwrap();
    assert_eq!(started.recv().await.as_deref(), Some("follow2"));
    permits.send(Ok(())).unwrap();

    let ActiveQueueResult::Completed { delivered, .. } = task.await.unwrap() else {
        panic!("queue should complete");
    };
    assert_eq!(
        delivered,
        [
            prompt("active", QueueKind::Steering),
            prompt("follow1", QueueKind::FollowUp),
            prompt("follow2", QueueKind::FollowUp),
        ]
    );
    assert_eq!(
        *timeline.lock().unwrap(),
        [
            "started:active",
            "finished:active",
            "started:follow1",
            "finished:follow1",
            "started:follow2",
            "finished:follow2",
        ]
    );
}

#[tokio::test]
async fn failure_restores_prompts_that_have_not_reached_the_runner() {
    let (runner, permits, mut started, _) = fake_runner();
    let (input_sender, mut input) = mpsc::unbounded_channel();
    let runner_ref = Arc::new(runner);
    let runner_clone = Arc::clone(&runner_ref);
    let task = tokio::spawn(async move {
        run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
    });

    started.recv().await.unwrap();
    input_sender
        .send(CoordinatorInput::Prompt(prompt("queued", QueueKind::FollowUp)))
        .unwrap();
    permits.send(Err("provider failed")).unwrap();

    assert_eq!(
        task.await.unwrap(),
        ActiveQueueResult::Failed {
            error: "provider failed",
            delivered: Vec::new(),
            restored: vec![prompt("queued", QueueKind::FollowUp)],
            deferred_commands: Vec::new(),
        }
    );
}

#[tokio::test]
async fn cancellation_restores_queue_and_retains_commands_for_idle_execution() {
    let (runner, _permits, mut started, timeline) = fake_runner();
    let (input_sender, mut input) = mpsc::unbounded_channel();
    let runner_ref = Arc::new(runner);
    let runner_clone = Arc::clone(&runner_ref);
    let task = tokio::spawn(async move {
        run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
    });

    started.recv().await.unwrap();
    input_sender
        .send(CoordinatorInput::Command("/model next".to_string()))
        .unwrap();
    input_sender
        .send(CoordinatorInput::Prompt(prompt("queued", QueueKind::FollowUp)))
        .unwrap();
    input_sender.send(CoordinatorInput::Cancel).unwrap();

    assert_eq!(
        task.await.unwrap(),
        ActiveQueueResult::Cancelled {
            delivered: Vec::new(),
            restored: vec![prompt("queued", QueueKind::FollowUp)],
            deferred_commands: vec!["/model next".to_string()],
            cancellation_error: None,
        }
    );
    assert_eq!(*timeline.lock().unwrap(), ["started:active", "cancelled"]);
}

#[tokio::test]
async fn slash_commands_queued_as_prompts_are_deferred_as_commands() {
    let (runner, permits, mut started, timeline) = fake_runner();
    let (input_sender, mut input) = mpsc::unbounded_channel();
    let runner_ref = Arc::new(runner);
    let runner_clone = Arc::clone(&runner_ref);
    let task = tokio::spawn(async move {
        run_active_queue(prompt("active", QueueKind::Steering), &mut input, &*runner_clone).await
    });

    started.recv().await.unwrap();
    input_sender
        .send(CoordinatorInput::Prompt(prompt("/reload", QueueKind::Steering)))
        .unwrap();
    permits.send(Ok(())).unwrap();

    let ActiveQueueResult::Completed {
        delivered,
        deferred_commands,
    } = task.await.unwrap()
    else {
        panic!("queue should complete");
    };

    assert_eq!(delivered, [prompt("active", QueueKind::Steering)]);
    assert_eq!(deferred_commands, ["/reload"]);
    assert_eq!(*timeline.lock().unwrap(), ["started:active", "finished:active"]);
}
