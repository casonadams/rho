use std::pin::Pin;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::Sleep;

use crate::engine::AgentEngine;
use crate::engine::runner::CancellationSignal;
use crate::error::Result;
use crate::ui::interactive::TranscriptItem;

struct InFlightRecap {
    cancel: CancellationSignal,
    rx: oneshot::Receiver<Option<String>>,
}

pub(crate) enum RecapEvent {
    Timer,
    Ready(Option<String>),
}

pub(crate) struct IdleRecapController {
    timer: Option<Pin<Box<Sleep>>>,
    in_flight: Option<InFlightRecap>,
    already_shown: bool,
}

impl Default for IdleRecapController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IdleRecapController {
    fn drop(&mut self) {
        self.cancel_and_disarm();
    }
}

impl IdleRecapController {
    pub(crate) fn new() -> Self {
        Self {
            timer: None,
            in_flight: None,
            already_shown: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.timer.is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(crate) fn cancel_and_disarm(&mut self) {
        self.timer = None;
        if let Some(in_flight) = self.in_flight.take() {
            in_flight.cancel.cancel();
        }
    }

    pub(crate) fn reset_idle(&mut self) {
        self.cancel_and_disarm();
        self.already_shown = false;
    }

    pub(crate) fn mark_shown(&mut self) {
        self.already_shown = true;
    }

    pub(crate) fn arm(&mut self, delay: Duration) {
        self.timer = Some(Box::pin(tokio::time::sleep(delay)));
    }

    pub(crate) fn arm_if_eligible(
        &mut self,
        enabled: bool,
        idle_seconds: u64,
        has_history: bool,
        editor_empty: bool,
        is_compacting: bool,
    ) {
        if !enabled || !has_history || !editor_empty || is_compacting || self.already_shown {
            return;
        }
        if self.timer.is_none() && self.in_flight.is_none() {
            self.arm(Duration::from_secs(idle_seconds));
        }
    }

    pub(crate) async fn next_event(&mut self) -> RecapEvent {
        tokio::select! {
            () = wait_timer_opt(&mut self.timer) => RecapEvent::Timer,
            res = wait_result_opt(&mut self.in_flight) => RecapEvent::Ready(res),
        }
    }

    pub(crate) async fn start_now(&mut self, engine: &AgentEngine) -> Result<bool> {
        self.cancel_and_disarm();
        let cancel = CancellationSignal::default();
        let Some(prepared) = engine.prepare_recap(cancel.clone()).await? else {
            return Ok(false);
        };
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let formatted = match prepared.execute(None).await {
                Ok(response) => rho_engine::engine::format_recap_text(&response.text),
                Err(_) => None,
            };
            let _ = tx.send(formatted);
        });
        self.in_flight = Some(InFlightRecap { cancel, rx });
        Ok(true)
    }
}

pub(crate) fn has_transcript_history(items: &[TranscriptItem]) -> bool {
    items.iter().any(|item| {
        matches!(
            item,
            TranscriptItem::UserMessage(_) | TranscriptItem::AssistantText(_) | TranscriptItem::Tool(_)
        )
    })
}

async fn wait_timer_opt(timer: &mut Option<Pin<Box<Sleep>>>) {
    match timer.as_mut() {
        Some(sleep) => {
            sleep.await;
            *timer = None;
        }
        None => std::future::pending().await,
    }
}

async fn wait_result_opt(in_flight: &mut Option<InFlightRecap>) -> Option<String> {
    let Some(active) = in_flight.as_mut() else {
        return std::future::pending().await;
    };
    let res = (&mut active.rx).await;
    *in_flight = None;
    res.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timer_arms_when_eligible_and_fires_after_delay() {
        let mut recap = IdleRecapController::new();
        recap.arm_if_eligible(true, 10, true, true, false);
        assert!(recap.is_armed());

        // Override with a short delay to verify timer firing in milliseconds.
        recap.arm(Duration::from_millis(15));
        let fired = tokio::select! {
            ev = recap.next_event() => matches!(ev, RecapEvent::Timer),
            _ = tokio::time::sleep(Duration::from_millis(200)) => false,
        };
        assert!(fired);
        assert!(!recap.is_armed());
    }

    #[tokio::test]
    async fn timer_does_not_arm_when_ineligible() {
        let mut recap = IdleRecapController::new();
        recap.arm_if_eligible(false, 10, true, true, false);
        assert!(!recap.is_armed());

        recap.arm_if_eligible(true, 10, false, true, false);
        assert!(!recap.is_armed());

        recap.arm_if_eligible(true, 10, true, false, false);
        assert!(!recap.is_armed());

        recap.arm_if_eligible(true, 10, true, true, true);
        assert!(!recap.is_armed());

        recap.mark_shown();
        recap.arm_if_eligible(true, 10, true, true, false);
        assert!(!recap.is_armed());
    }

    #[tokio::test]
    async fn cancel_and_disarm_aborts_timer_and_inflight_signal() {
        let mut recap = IdleRecapController::new();
        recap.arm_if_eligible(true, 10, true, true, false);
        assert!(recap.is_armed());

        let cancel = CancellationSignal::default();
        let (_tx, rx) = oneshot::channel();
        recap.in_flight = Some(InFlightRecap {
            cancel: cancel.clone(),
            rx,
        });

        recap.cancel_and_disarm();
        assert!(!recap.is_armed());
        assert!(!recap.is_in_flight());
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn drop_cancels_inflight_recap() {
        let cancel = CancellationSignal::default();
        let (_tx, rx) = oneshot::channel();
        let recap = IdleRecapController {
            timer: None,
            in_flight: Some(InFlightRecap {
                cancel: cancel.clone(),
                rx,
            }),
            already_shown: false,
        };
        assert!(!cancel.is_cancelled());
        drop(recap);
        assert!(cancel.is_cancelled());
    }
}
