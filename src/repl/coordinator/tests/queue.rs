use std::sync::Arc;
use tokio::sync::mpsc;

use super::{ActiveQueueResult, CoordinatorInput, fake_runner, prompt, run_active_queue};
use crate::ui::interactive::QueueKind;

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
