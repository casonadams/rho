use std::sync::Arc;
use tokio::sync::mpsc;

use super::{ActiveQueueResult, CoordinatorInput, fake_runner, prompt, run_active_queue};
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::interactive::QueueKind;

async fn drive_steering_sequence(
    input_sender: &mpsc::UnboundedSender<CoordinatorInput>,
    permits: &mpsc::UnboundedSender<Result<(), &'static str>>,
) {
    let _ = input_sender.send(CoordinatorInput::Prompt(prompt("steer", QueueKind::Steering)));
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let _ = input_sender.send(CoordinatorInput::Prompt(prompt("follow", QueueKind::FollowUp)));
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let _ = permits.send(Ok(()));
    let _ = permits.send(Ok(()));
}

fn assert_steering_results(delivered: &[crate::ui::interactive::QueuedMessage], timeline: &[String]) {
    assert_eq!(
        delivered,
        [
            prompt("steer", QueueKind::Steering),
            prompt("active", QueueKind::Steering),
            prompt("follow", QueueKind::FollowUp)
        ]
    );
    let expected = [
        "started:active",
        "steered:steer",
        "finished:active",
        "started:follow",
        "finished:follow",
    ];
    assert_eq!(timeline, expected);
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
    drive_steering_sequence(&input_sender, &permits).await;
    assert_eq!(started.recv().await.as_deref(), Some("follow"));

    let ActiveQueueResult::Completed { delivered, .. } = task.await.unwrap() else {
        panic!()
    };
    assert_steering_results(&delivered, &timeline.lock().unwrap());
}

#[tokio::test]
async fn shared_steering_queue_records_and_drains_consumed_prompts() {
    let queue = SharedSteeringQueue::new(crate::engine::runner::QueueMode::All);
    queue.enqueue("first steer".to_string());
    queue.enqueue("second steer".to_string());

    assert!(queue.consumed().is_empty());

    let polled = crate::engine::runner::SteeringQueueProvider::poll_steering(&queue).await;
    assert_eq!(polled, vec!["first steer", "second steer"]);
    assert_eq!(queue.consumed(), vec!["first steer", "second steer"]);

    let taken = queue.take_consumed();
    assert_eq!(taken, vec!["first steer", "second steer"]);
    assert!(queue.consumed().is_empty());

    queue.enqueue("third steer".to_string());
    queue.clear();
    let polled_after_clear = crate::engine::runner::SteeringQueueProvider::poll_steering(&queue).await;
    assert!(polled_after_clear.is_empty());
}
