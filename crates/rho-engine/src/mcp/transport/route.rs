use super::super::types::{JsonRpcError, JsonRpcResponse};
use rho_harness_core::error::AppError;
use serde_json::Value;

/// JSON-RPC 2.0 reserved code for a method the receiver does not implement.
const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, PartialEq)]
pub(crate) enum Routed {
    Notification(String),
    Request { id: Value, method: String },
    Response(Value, Result<Value, JsonRpcError>),
}

pub(crate) fn route_message(message: Value) -> Option<Routed> {
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        let method = method.to_string();
        return Some(match message.get("id") {
            Some(id) => Routed::Request { id: id.clone(), method },
            None => Routed::Notification(method),
        });
    }
    let response = serde_json::from_value::<JsonRpcResponse>(message).ok()?;
    let outcome = match response.error {
        Some(error) => Err(error),
        None => Ok(response.result.unwrap_or(Value::Null)),
    };
    Some(Routed::Response(response.id, outcome))
}

/// Answers a server-initiated request: MCP requires an empty result for `ping`; nothing else is supported.
pub(crate) fn reply_to_server_request(id: Value, method: &str) -> JsonRpcResponse {
    if method == "ping" {
        return JsonRpcResponse::success(id, Value::Object(serde_json::Map::new()));
    }
    JsonRpcResponse::failure(
        id,
        JsonRpcError {
            code: METHOD_NOT_FOUND,
            message: format!("Method not found: {method}"),
            data: None,
        },
    )
}

pub(crate) fn server_error(method: &str, error: &JsonRpcError) -> AppError {
    AppError::Mcp(format!(
        "MCP error from {method}: {} (code {})",
        error.message, error.code
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use test_case::test_case;

    #[test_case(
        json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }),
        Some(Routed::Notification("notifications/tools/list_changed".to_string()));
        "notification"
    )]
    #[test_case(
        json!({ "jsonrpc": "2.0", "id": "a", "method": "ping" }),
        Some(Routed::Request { id: json!("a"), method: "ping".to_string() });
        "request"
    )]
    #[test_case(
        json!({ "jsonrpc": "2.0", "id": 3, "result": { "ok": true } }),
        Some(Routed::Response(json!(3), Ok(json!({ "ok": true }))));
        "result response"
    )]
    #[test_case(
        json!({ "jsonrpc": "2.0", "id": 4 }),
        Some(Routed::Response(json!(4), Ok(Value::Null)));
        "response without result"
    )]
    #[test_case(
        json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32602, "message": "Bad params" } }),
        Some(Routed::Response(
            json!(null),
            Err(JsonRpcError { code: -32602, message: "Bad params".to_string(), data: None })
        ));
        "error response"
    )]
    #[test_case(json!({ "jsonrpc": "2.0", "result": {} }), None; "result without id")]
    #[test_case(json!("not an object"), None; "not an object")]
    fn messages_are_classified_by_shape(message: Value, expected: Option<Routed>) {
        assert_eq!(route_message(message), expected);
    }

    #[test_case("ping", json!({ "jsonrpc": "2.0", "id": "s1", "result": {} }); "ping")]
    #[test_case(
        "roots/list",
        json!({ "jsonrpc": "2.0", "id": "s1", "error": { "code": -32601, "message": "Method not found: roots/list" } });
        "unsupported method"
    )]
    fn server_requests_get_ping_results_or_method_not_found(method: &str, expected: Value) {
        let reply = reply_to_server_request(json!("s1"), method);
        assert_eq!(serde_json::to_value(reply).unwrap(), expected);
    }
}
