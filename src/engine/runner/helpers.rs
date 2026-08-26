use crate::session::SessionManager;
use serde_json::Value;

use super::run_turn::TerminalSinkState;

pub(super) fn clear_spinner(state: &mut TerminalSinkState) {
    if let Some(spinner) = state.spinner.take() {
        spinner.finish_and_clear();
    }
}

pub(super) fn needs_approval(state: &TerminalSinkState, class: &super::super::tools::policy::ExecutionClass) -> bool {
    !state.auto_approve && !class.allows_without_approval()
}

pub(super) fn redact_value(session: &SessionManager, value: &Value) -> Value {
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
