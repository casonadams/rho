use crate::ui::interactive::{InteractionResponder, InteractionResponse};
use rho_harness_core::a2ui::A2uiModalResult;
use tokio::sync::oneshot;

pub enum PendingModal {
    Interaction(InteractionResponder),
    A2ui(oneshot::Sender<A2uiModalResult>),
}

impl PendingModal {
    pub(crate) fn respond_interaction(self, response: InteractionResponse) {
        if let Self::Interaction(responder) = self {
            let _ = responder.respond(response);
        }
    }

    pub(crate) fn respond_a2ui(self, result: A2uiModalResult) {
        if let Self::A2ui(tx) = self {
            let _ = tx.send(result);
        }
    }
}

pub(crate) fn is_input_trigger(label: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "with reason",
        "with feedback",
        "custom answer",
        "custom input",
        "Type something",
        "Type a custom",
        "Deny with reason",
        "Accept input",
    ];
    PATTERNS.iter().any(|p| label.contains(p))
}

pub(crate) fn prompt_label_for(label: &str) -> &'static str {
    const REASON_PATTERNS: &[&str] = &["reason", "feedback", "Permission", "Approve"];
    if REASON_PATTERNS.iter().any(|p| label.contains(p)) {
        "reason"
    } else {
        "answer"
    }
}

pub(crate) fn build_enter_response(custom: String, input_option: Option<usize>) -> InteractionResponse {
    if let Some(index) = input_option {
        InteractionResponse::SelectedWithInput { index, text: custom }
    } else if !custom.is_empty() {
        InteractionResponse::Custom(custom)
    } else {
        InteractionResponse::Cancelled
    }
}
