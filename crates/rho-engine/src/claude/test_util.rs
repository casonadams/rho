use super::http::ClaudeAuth;
use super::request::build_request_body;
use crate::provider::adapter::ModelCompletionRequest;
use serde_json::Value;

pub(crate) fn login_body(model: &str, thinking: Option<&str>, request: &ModelCompletionRequest) -> Value {
    build_request_body(model, thinking, request, ClaudeAuth::Login).unwrap()
}

/// Whether the last content part of each message carries an ephemeral cache breakpoint.
pub(crate) fn message_marks(body: &Value) -> Vec<bool> {
    let marked = |message: &Value| {
        let last = message["content"].as_array().unwrap().last().unwrap();
        last["cache_control"]["type"] == "ephemeral"
    };
    body["messages"].as_array().unwrap().iter().map(marked).collect()
}
