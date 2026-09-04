use std::sync::Arc;
use tokio::sync::mpsc;

use super::{ActiveQueueResult, CoordinatorInput, fake_runner, prompt, run_active_queue};
use crate::ui::interactive::QueueKind;

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
