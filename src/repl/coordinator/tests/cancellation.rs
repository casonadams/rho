use std::sync::Arc;
use tokio::sync::mpsc;

use super::{ActiveQueueResult, CoordinatorInput, fake_runner, prompt, run_active_queue};
use crate::ui::interactive::QueueKind;

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
