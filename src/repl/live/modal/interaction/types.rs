use crate::ui::interactive::{InteractionResponder, InteractionResponse};

pub struct PendingModal {
    pub(crate) responder: InteractionResponder,
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
    if label.contains("reason")
        || label.contains("feedback")
        || label.contains("Permission")
        || label.contains("Approve")
    {
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
