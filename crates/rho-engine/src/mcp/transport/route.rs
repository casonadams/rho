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

    #[test]
    fn messages_are_classified_by_shape() {
        let error = JsonRpcError {
            code: -32602,
            message: "Bad params".to_string(),
            data: None,
        };
        let cases = [
            (
                json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }),
                Some(Routed::Notification("notifications/tools/list_changed".to_string())),
            ),
            (
                json!({ "jsonrpc": "2.0", "id": "a", "method": "ping" }),
                Some(Routed::Request {
                    id: json!("a"),
                    method: "ping".to_string(),
                }),
            ),
            (
                json!({ "jsonrpc": "2.0", "id": 3, "result": { "ok": true } }),
                Some(Routed::Response(json!(3), Ok(json!({ "ok": true })))),
            ),
            (
                json!({ "jsonrpc": "2.0", "id": 4 }),
                Some(Routed::Response(json!(4), Ok(Value::Null))),
            ),
            (
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32602, "message": "Bad params" } }),
                Some(Routed::Response(json!(null), Err(error))),
            ),
            (json!({ "jsonrpc": "2.0", "result": {} }), None),
            (json!("not an object"), None),
        ];
        for (message, expected) in cases {
            assert_eq!(route_message(message.clone()), expected, "{message}");
        }
    }

    #[test]
    fn server_requests_get_ping_results_or_method_not_found() {
        let cases = [
            ("ping", json!({ "jsonrpc": "2.0", "id": "s1", "result": {} })),
            (
                "roots/list",
                json!({ "jsonrpc": "2.0", "id": "s1", "error": { "code": -32601, "message": "Method not found: roots/list" } }),
            ),
        ];
        for (method, expected) in cases {
            let reply = reply_to_server_request(json!("s1"), method);
            assert_eq!(serde_json::to_value(reply).unwrap(), expected, "{method}");
        }
    }
}
