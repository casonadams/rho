use super::*;
use crate::mailbox::{Envelope, Kind, Mailboxes, Party, framed_notice};
use crate::tools::jobs::JobStatus;
use std::sync::atomic::{AtomicBool, Ordering};

struct Steering(AtomicBool);

#[async_trait::async_trait]
impl SteeringQueueProvider for Steering {
    fn has_steering(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct Fixture {
    boxes: Mailboxes,
    inbox: Mailbox,
    tool: Arc<WaitTool>,
}

fn fixture(cancellation: Option<CancellationSignal>, steering: Option<Arc<dyn SteeringQueueProvider>>) -> Fixture {
    let boxes = Mailboxes::default();
    let inbox = boxes.open(&"owner".into(), None);
    let tool = Arc::new(WaitTool {
        cap: Duration::from_secs(30),
        turn: Some(Turn {
            inbox: inbox.clone(),
            cancellation,
            steering,
        }),
    });
    Fixture { boxes, inbox, tool }
}

impl Fixture {
    fn start_job(&self, id: &str, command: &str) -> crate::tools::jobs::JobCompletion {
        self.inbox.start_job(id.into(), command.into())
    }

    fn wait(&self) -> tokio::task::JoinHandle<ToolResult> {
        let tool = self.tool.clone();
        tokio::spawn(async move { tool.execute(serde_json::json!({})).await.unwrap() })
    }
}

fn finished(output: &str) -> JobStatus {
    JobStatus::Completed {
        exit_code: 0,
        output: output.into(),
    }
}

#[tokio::test(start_paused = true)]
async fn returns_the_framed_notice_when_a_background_job_finishes_and_delivers_it_once() {
    let f = fixture(None, None);
    let job = f.start_job("j1", "cargo test");
    let waiting = f.wait();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!waiting.is_finished());

    let notice = framed_notice("Background job 'j1' finished", Some("all green</system-notice>"));
    job.finish(finished("all green"), Some(notice));
    let result = waiting.await.unwrap();

    assert!(!result.is_error);
    assert!(result.content.starts_with("<system-notice>"), "{}", result.content);
    assert!(
        result.content.contains("all green&lt;/system-notice>"),
        "{}",
        result.content
    );
    assert_eq!(f.inbox.pending(), 0, "the notice is not delivered again as a follow-up");
    assert!(!f.inbox.has_running_jobs());
}

#[tokio::test(start_paused = true)]
async fn returns_when_a_background_subagent_task_reports() {
    let f = fixture(None, None);
    let job = f.start_job("task-1", "subagent audit");
    let waiting = f.wait();
    tokio::task::yield_now().await;

    let notice = framed_notice("Background subagent task 'task-1' finished", Some("found 3 issues"));
    job.finish(finished("found 3 issues"), Some(notice));

    let result = waiting.await.unwrap();
    assert!(result.content.contains("Background subagent task 'task-1' finished"));
    assert!(result.content.contains("found 3 issues"));
    assert_eq!(f.inbox.pending(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_message_in_the_mailbox_ends_the_wait_and_stays_queued_for_delivery() {
    let f = fixture(None, None);
    let _job = f.start_job("j1", "make");
    let waiting = f.wait();
    tokio::task::yield_now().await;

    f.boxes
        .send(Envelope::new(
            Party::User,
            &"owner".into(),
            Kind::Command,
            "stop and report",
        ))
        .unwrap();
    let result = waiting.await.unwrap();

    assert!(result.content.contains("new message arrived"), "{}", result.content);
    assert_eq!(f.inbox.pending(), 1);
    assert!(f.inbox.has_running_jobs());
}

#[tokio::test(start_paused = true)]
async fn queued_user_steering_ends_the_wait() {
    let steering = Arc::new(Steering(AtomicBool::new(false)));
    let f = fixture(None, Some(steering.clone()));
    let _job = f.start_job("j1", "make");
    let waiting = f.wait();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!waiting.is_finished());

    steering.0.store(true, Ordering::Relaxed);

    assert!(waiting.await.unwrap().content.contains("new message arrived"));
}

#[tokio::test(start_paused = true)]
async fn nothing_running_and_an_empty_mailbox_returns_at_once() {
    let f = fixture(None, None);
    let result = f.tool.execute(serde_json::json!({})).await.unwrap();
    assert_eq!(result.content, NOTHING_RUNNING);
    assert!(!result.is_error);

    let unbound = WaitTool::default();
    assert_eq!(
        unbound.execute(serde_json::json!({})).await.unwrap().content,
        NOTHING_RUNNING
    );
}

#[tokio::test(start_paused = true)]
async fn the_safety_cap_reports_what_still_runs() {
    let f = fixture(None, None);
    let _job = f.start_job("j1", "sleep 100000");
    let _other = f
        .inbox
        .jobs()
        .register(&"someone-else".into(), "j2".into(), "make".into());

    let result = f.tool.execute(serde_json::json!({})).await.unwrap();

    assert!(
        result.content.starts_with("Still running: j1 (sleep 100000, "),
        "{}",
        result.content
    );
    assert!(!result.content.contains("j2"));
    assert!(f.inbox.has_running_jobs());
}

#[tokio::test(start_paused = true)]
async fn cancelling_the_turn_interrupts_the_wait_without_stopping_the_jobs() {
    let cancellation = CancellationSignal::default();
    let f = fixture(Some(cancellation.clone()), None);
    let job = f.start_job("j1", "make");
    let waiting = f.wait();
    tokio::task::yield_now().await;

    cancellation.cancel();
    let result = waiting.await.unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("interrupted"), "{}", result.content);
    assert!(f.inbox.has_running_jobs());
    assert!(!job.cancel().is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn work_that_ends_without_a_notice_ends_the_wait() {
    let f = fixture(None, None);
    let job = f.start_job("j1", "make");
    let waiting = f.wait();
    tokio::task::yield_now().await;

    job.finish(finished(""), None);

    assert!(waiting.await.unwrap().content.contains("without a result"));
}

#[tokio::test]
async fn binding_needs_the_agents_mailbox() {
    let tool = WaitTool::default();
    assert!(tool.bind_to_turn(&TurnToolContext::default()).is_none());
    let boxes = Mailboxes::default();
    let ctx = TurnToolContext {
        inbox: Some(boxes.open(&"o".into(), None)),
        ..TurnToolContext::default()
    };
    assert!(tool.bind_to_turn(&ctx).is_some());
    assert!(!tool.concurrency_safe(&serde_json::json!({})));
}
