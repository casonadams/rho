use rho_core::policy::ExecutionClass;
use rho_core::session::SessionManager;
use serde_json::Value;

use super::sink::TerminalSinkState;

pub fn clear_spinner(state: &mut TerminalSinkState) {
    if let Some(spinner) = state.spinner.take() {
        spinner.finish_and_clear();
    }
}

pub fn needs_approval(state: &TerminalSinkState, class: &ExecutionClass) -> bool {
    !state.auto_approve && !class.allows_without_approval()
}

pub fn redact_value(session: &SessionManager, value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(session.redact_credentials(value)),
        Value::Array(values) => Value::Array(values.iter().map(|value| redact_value(session, value)).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), redact_value(session, value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

pub fn redact_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if ["api_key", "access_token", "refresh_token", "authorization", "bearer "]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        "sensitive upstream detail redacted".to_string()
    } else {
        value.to_string()
    }
}
