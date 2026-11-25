use tokio::sync::oneshot;

use crate::engine::AgentEngine;
use crate::engine::runner::CancellationSignal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PredictionResult {
    pub expected_draft: String,
    pub text: Option<String>,
}

struct InFlightPrediction {
    cancel: CancellationSignal,
    rx: oneshot::Receiver<PredictionResult>,
}

pub(crate) struct IdlePredictionController {
    in_flight: Option<InFlightPrediction>,
    already_predicted: bool,
}

impl Default for IdlePredictionController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IdlePredictionController {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl IdlePredictionController {
    pub(crate) fn new() -> Self {
        Self {
            in_flight: None,
            already_predicted: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    fn cancel_in_flight(&mut self) {
        if let Some(in_flight) = self.in_flight.take() {
            in_flight.cancel.cancel();
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.cancel_in_flight();
    }

    pub(crate) async fn start_if_eligible(
        &mut self,
        engine: &AgentEngine,
        enabled: bool,
        has_history: bool,
        editor_empty: bool,
        has_modal: bool,
    ) {
        if !enabled || !has_history || !editor_empty || has_modal || self.already_predicted {
            return;
        }
        self.already_predicted = true;
        let cancel = CancellationSignal::default();
        let Ok(Some(prepared)) = engine.prepare_prompt_prediction(cancel.clone()).await else {
            return;
        };
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let text = match prepared.execute(None).await {
                Ok(resp) => rho_engine::engine::format_prompt_prediction(&resp.text),
                Err(_) => None,
            };
            let _ = tx.send(PredictionResult {
                expected_draft: String::new(),
                text,
            });
        });
        self.in_flight = Some(InFlightPrediction { cancel, rx });
    }

    pub(crate) async fn next_result(&mut self) -> PredictionResult {
        wait_in_flight(&mut self.in_flight).await
    }
}

async fn wait_in_flight(in_flight: &mut Option<InFlightPrediction>) -> PredictionResult {
    let Some(active) = in_flight.as_mut() else {
        return std::future::pending().await;
    };
    let res = (&mut active.rx).await;
    *in_flight = None;
    res.unwrap_or_else(|_| PredictionResult {
        expected_draft: String::new(),
        text: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_aborts_inflight_signal() {
        let cancel = CancellationSignal::default();
        let (_tx, rx) = oneshot::channel();
        let mut controller = IdlePredictionController {
            in_flight: Some(InFlightPrediction {
                cancel: cancel.clone(),
                rx,
            }),
            already_predicted: true,
        };
        assert!(controller.is_in_flight());
        assert!(!cancel.is_cancelled());

        controller.cancel();
        assert!(!controller.is_in_flight());
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn drop_cancels_inflight_prediction() {
        let cancel = CancellationSignal::default();
        let (_tx, rx) = oneshot::channel();
        let controller = IdlePredictionController {
            in_flight: Some(InFlightPrediction {
                cancel: cancel.clone(),
                rx,
            }),
            already_predicted: true,
        };
        assert!(!cancel.is_cancelled());
        drop(controller);
        assert!(cancel.is_cancelled());
    }
}
