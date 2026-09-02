use super::convert::*;
use super::types::*;
use rig::agent::hook::{
    CompletionCallAction, InvalidToolCallAction, ObservationAction, ToolCallAction, ToolResultAction,
};
use serde_json::json;

#[test]
fn json_rpc_request_response_serde() {
    let req = JsonRpcRequest::new(1, "hook/tool_call", json!({"tool_name": "bash"}));
    let req_json = serde_json::to_string(&req).expect("serialize request");
    let req_de: JsonRpcRequest = serde_json::from_str(&req_json).expect("deserialize request");
    assert_eq!(req, req_de);

    let res = JsonRpcResponse::ok(1, json!({"action": "continue"}));
    let res_json = serde_json::to_string(&res).expect("serialize response");
    let res_de: JsonRpcResponse = serde_json::from_str(&res_json).expect("deserialize response");
    assert_eq!(res, res_de);

    let err_res = JsonRpcResponse::err(2, -32600, "Invalid Request");
    let err_json = serde_json::to_string(&err_res).expect("serialize error");
    let err_de: JsonRpcResponse = serde_json::from_str(&err_json).expect("deserialize error");
    assert_eq!(err_res, err_de);
}

#[test]
fn plugin_events_serde_roundtrip() {
    let events = vec![
        PluginEvent::CompletionCall {
            turn: 1,
            prompt: json!({"role": "user", "content": "hi"}),
            history: vec![],
        },
        PluginEvent::CompletionResponse {
            prompt: json!("hi"),
            response: json!({"content": "hello"}),
        },
        PluginEvent::ToolCall {
            tool_name: "bash".to_string(),
            args: json!({"command": "ls"}),
        },
        PluginEvent::ToolResult {
            tool_name: "bash".to_string(),
            args: json!({"command": "ls"}),
            output: "file.txt".to_string(),
            is_error: false,
        },
        PluginEvent::InvalidToolCall {
            tool_name: "shell".to_string(),
            args: json!({"command": "ls"}),
            available_tools: vec!["bash".to_string()],
        },
        PluginEvent::TextDelta {
            delta: "hello ".to_string(),
        },
        PluginEvent::ReasoningDelta {
            delta: "thinking...".to_string(),
        },
    ];

    for event in events {
        let serialized = serde_json::to_string(&event).expect("serialize event");
        let deserialized: PluginEvent = serde_json::from_str(&serialized).expect("deserialize event");
        assert_eq!(event, deserialized);
    }
}

#[test]
fn plugin_flow_serde_roundtrip() {
    let flows = vec![
        PluginFlow::Continue,
        PluginFlow::Skip {
            reason: "denied".to_string(),
        },
        PluginFlow::RewriteArgs {
            args: json!({"command": "safe"}),
        },
        PluginFlow::RewriteResult {
            result: "sanitized".to_string(),
        },
        PluginFlow::OverrideRequest {
            request: RequestPatchPayload {
                preamble: Some("system".to_string()),
                temperature: Some(0.5),
                max_tokens: Some(100),
                active_tools: Some(vec!["bash".to_string()]),
                tool_choice: None,
                additional_params: Some(json!({"top_p": 0.9})),
                extra_context: Some(vec![DocumentPayload {
                    id: "doc1".to_string(),
                    text: "content".to_string(),
                }]),
                history: None,
            },
        },
        PluginFlow::Repair {
            tool_name: "bash".to_string(),
        },
        PluginFlow::Retry {
            feedback: "try again".to_string(),
        },
        PluginFlow::Terminate {
            reason: "aborted".to_string(),
        },
    ];

    for flow in flows {
        let serialized = serde_json::to_string(&flow).expect("serialize flow");
        let deserialized: PluginFlow = serde_json::from_str(&serialized).expect("deserialize flow");
        assert_eq!(flow, deserialized);
    }
}

#[test]
fn tool_call_action_conversions() {
    assert_eq!(flow_to_tool_call_action(PluginFlow::Continue), ToolCallAction::run());
    assert_eq!(
        flow_to_tool_call_action(PluginFlow::Skip {
            reason: "blocked".into()
        }),
        ToolCallAction::skip("blocked")
    );
    assert_eq!(
        flow_to_tool_call_action(PluginFlow::RewriteArgs {
            args: json!({"cmd": "echo"})
        }),
        ToolCallAction::rewrite(json!({"cmd": "echo"}))
    );
    assert_eq!(
        flow_to_tool_call_action(PluginFlow::Terminate { reason: "stop".into() }),
        ToolCallAction::stop("stop")
    );
}

#[test]
fn tool_result_action_conversions() {
    assert_eq!(
        flow_to_tool_result_action(PluginFlow::Continue),
        ToolResultAction::keep()
    );
    assert_eq!(
        flow_to_tool_result_action(PluginFlow::RewriteResult {
            result: "sanitized".into()
        }),
        ToolResultAction::rewrite("sanitized")
    );
    assert_eq!(
        flow_to_tool_result_action(PluginFlow::Terminate { reason: "stop".into() }),
        ToolResultAction::stop("stop")
    );
}

#[test]
fn invalid_tool_call_action_conversions() {
    assert_eq!(
        flow_to_invalid_tool_call_action(PluginFlow::Continue),
        InvalidToolCallAction::Fail
    );
    assert_eq!(
        flow_to_invalid_tool_call_action(PluginFlow::Repair {
            tool_name: "bash".into()
        }),
        InvalidToolCallAction::Repair {
            tool_name: "bash".into()
        }
    );
    assert_eq!(
        flow_to_invalid_tool_call_action(PluginFlow::Retry { feedback: "fix".into() }),
        InvalidToolCallAction::Retry { feedback: "fix".into() }
    );
    assert_eq!(
        flow_to_invalid_tool_call_action(PluginFlow::Skip { reason: "skip".into() }),
        InvalidToolCallAction::Skip { reason: "skip".into() }
    );
    assert_eq!(
        flow_to_invalid_tool_call_action(PluginFlow::Terminate { reason: "stop".into() }),
        InvalidToolCallAction::Stop { reason: "stop".into() }
    );
}

#[test]
fn completion_call_and_observation_conversions() {
    assert_eq!(
        flow_to_completion_call_action(PluginFlow::Continue),
        CompletionCallAction::continue_run()
    );
    assert_eq!(
        flow_to_completion_call_action(PluginFlow::Terminate { reason: "stop".into() }),
        CompletionCallAction::stop("stop")
    );

    let patch_flow = PluginFlow::OverrideRequest {
        request: RequestPatchPayload {
            preamble: Some("hello".into()),
            temperature: Some(0.2),
            max_tokens: Some(50),
            active_tools: Some(vec!["bash".into()]),
            tool_choice: None,
            additional_params: Some(json!({"seed": 42})),
            extra_context: None,
            history: None,
        },
    };
    match flow_to_completion_call_action(patch_flow) {
        CompletionCallAction::Patch(patch) => {
            assert_eq!(patch.preamble, Some("hello".into()));
            assert_eq!(patch.temperature, Some(0.2));
            assert_eq!(patch.max_tokens, Some(50));
            assert_eq!(patch.active_tools, Some(vec!["bash".into()]));
        }
        _ => panic!("Expected CompletionCallAction::Patch"),
    }

    assert_eq!(
        flow_to_observation_action(PluginFlow::Continue),
        ObservationAction::continue_run()
    );
    assert_eq!(
        flow_to_observation_action(PluginFlow::Terminate { reason: "stop".into() }),
        ObservationAction::stop("stop")
    );
}
