use std::task::Poll;

use tokio::sync::mpsc;

use crate::repl::ReplSession;
pub(crate) use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{QueuedMessage, TerminalController, UiEvent};

pub struct LiveIo<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
    pub input: &'a mut TerminalInputReader,
}

pub(crate) async fn wait_quota(rx: &mut tokio::sync::watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

pub(crate) enum FrameOrUi {
    Frame,
    Ui(crate::ui::interactive::UiEvent),
}

pub(crate) async fn next_frame_or_ui(
    frame: &mut tokio::time::Interval,
    ui: &mut mpsc::UnboundedReceiver<UiEvent>,
) -> FrameOrUi {
    std::future::poll_fn(|cx| {
        if frame.poll_tick(cx).is_ready() {
            return Poll::Ready(FrameOrUi::Frame);
        }
        match ui.poll_recv(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(FrameOrUi::Ui(ev)),
            _ => Poll::Pending,
        }
    })
    .await
}

impl<'a, B: crate::ui::interactive::TerminalBackend> LiveIo<'a, B> {
    pub fn suspend_for<R, F: FnOnce() -> R>(&mut self, action: F) -> crate::error::Result<R> {
        let mut paused = self.input.pause()?;
        paused.drain();
        self.controller.suspend()?;
        let result = action();
        let controller_res = self.controller.resume();
        let input_res = paused.resume();
        self.input.drain();
        controller_res?;
        input_res?;
        Ok(result)
    }

    pub async fn suspend_for_async<R, Fut: std::future::Future<Output = R>, F: FnOnce() -> Fut>(
        &mut self,
        action: F,
    ) -> crate::error::Result<R> {
        let mut paused = self.input.pause()?;
        paused.drain();
        self.controller.suspend()?;
        let result = action().await;
        let controller_res = self.controller.resume();
        let input_res = paused.resume();
        self.input.drain();
        controller_res?;
        input_res?;
        Ok(result)
    }
}

pub struct EditorResources<'a> {
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
}

pub struct LiveMessage<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub message: QueuedMessage,
}

pub struct ActiveTurn<'a, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub prompt: &'a str,
}

pub struct IdleContext<'a, 'b, B: crate::ui::interactive::TerminalBackend = crate::ui::interactive::CrosstermBackend> {
    pub io: LiveIo<'a, B>,
    pub editor: EditorResources<'a>,
    pub session: &'b mut ReplSession,
    pub engine: &'b mut crate::engine::AgentEngine,
}

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

#[cfg(test)]
mod tests {
    use super::{FrameOrUi, next_frame_or_ui};
    use crate::ui::interactive::UiEvent;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[derive(Clone, Copy)]
    enum Expect {
        Frame,
        ToolEnd,
    }

    struct Scenario {
        period: Duration,
        settle: Option<Duration>,
        queue_tool_end: bool,
        sender_open: bool,
        expected: &'static [Expect],
    }

    const SCENARIOS: [Scenario; 2] = [
        Scenario {
            period: Duration::from_secs(60),
            settle: Some(Duration::from_millis(1)),
            queue_tool_end: true,
            sender_open: true,
            expected: &[Expect::Frame, Expect::ToolEnd],
        },
        Scenario {
            period: Duration::from_millis(1),
            settle: None,
            queue_tool_end: false,
            sender_open: false,
            expected: &[Expect::Frame, Expect::Frame],
        },
    ];

    fn matches_expect(got: &FrameOrUi, want: Expect) -> bool {
        matches!(
            (got, want),
            (FrameOrUi::Frame, Expect::Frame) | (FrameOrUi::Ui(UiEvent::ToolEnd), Expect::ToolEnd)
        )
    }

    #[tokio::test]
    async fn next_frame_or_ui_follows_scenarios() {
        for scenario in SCENARIOS {
            let mut frame = tokio::time::interval(scenario.period);
            if let Some(settle) = scenario.settle {
                tokio::time::sleep(settle).await;
            }
            let (tx, mut rx) = mpsc::unbounded_channel::<UiEvent>();
            if scenario.queue_tool_end {
                tx.send(UiEvent::ToolEnd).unwrap();
            }
            let _sender = scenario.sender_open.then_some(tx);
            for want in scenario.expected {
                assert!(matches_expect(&next_frame_or_ui(&mut frame, &mut rx).await, *want));
            }
        }
    }
}
