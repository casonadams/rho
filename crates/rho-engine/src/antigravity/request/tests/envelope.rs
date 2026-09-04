use super::*;
use rig::completion::ToolDefinition;
use rig::message::UserContent;

#[test]
fn request_envelope_has_project_model_and_agent_shape() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hello")],
    }]);
    let body = build_request_body(target("proj-1", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();

    assert_eq!(body["project"], "proj-1");
    assert_eq!(body["model"], "gemini-3.8-flash-low");
    assert_eq!(body["requestType"], "agent");
    assert_eq!(body["userAgent"], "antigravity");
    assert_eq!(body["request"]["systemInstruction"]["role"], "user");
    assert_eq!(
        body["request"]["systemInstruction"]["parts"][0]["text"],
        "system prompt"
    );
    assert_eq!(body["request"]["contents"][0]["role"], "user");
    assert_eq!(body["request"]["contents"][0]["parts"][0]["text"], "hello");
    // Gemini thinking config off by default.
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 65536);
}

#[test]
fn unsigned_tool_calls_flatten_to_observations_on_gemini_3() {
    let tool_call = rig::message::ToolCall {
        id: rig::message::ToolCallId::new("call-1").unwrap(),
        provider: None,
        function: rig::message::ToolFunction {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        },
        signature: None,
        additional_params: None,
    };
    let tool_result = rig::message::ToolResult {
        call: rig::message::ToolCallId::new("call-1").unwrap(),
        provider: None,
        name: "read_file".to_string(),
        content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(
            "file body",
        ))],
    };
    let history = vec![
        Message::User {
            content: vec![UserContent::text("read it")],
        },
        Message::Assistant {
            id: None,
            content: vec![rig::message::AssistantContent::ToolCall(tool_call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(tool_result)],
        },
    ];
    let request = minimal_request(history);
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();

    // The empty assistant turn vanishes and the result is replayed as a user
    // observation (merged into the previous user turn, pi parity). No
    // functionCall may appear anywhere on the wire.
    let serialized = body.to_string();
    assert!(!serialized.contains("functionCall"));
    let observation = contents[0]["parts"][1]["text"].as_str().unwrap();
    assert!(observation.contains("[Observation from `read_file`"));
    assert!(observation.contains("file body"));

    // Same history on Claude replays a real functionCall + functionResponse.
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();
    assert!(contents[1]["parts"][0].get("functionCall").is_some());
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "read_file");
}

#[test]
fn signed_tool_calls_replay_function_calls_on_gemini_3() {
    let tool_call = rig::message::ToolCall {
        id: rig::message::ToolCallId::new("call-1").unwrap(),
        provider: None,
        function: rig::message::ToolFunction {
            name: "read_file".to_string(),
            arguments: serde_json::json!({}),
        },
        signature: Some("c2lnbmF0dXJl".to_string()),
        additional_params: None,
    };
    let tool_result = rig::message::ToolResult {
        call: rig::message::ToolCallId::new("call-1").unwrap(),
        provider: None,
        name: "read_file".to_string(),
        content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new("ok"))],
    };
    let history = vec![
        Message::User {
            content: vec![UserContent::text("read it")],
        },
        Message::Assistant {
            id: None,
            content: vec![rig::message::AssistantContent::ToolCall(tool_call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(tool_result)],
        },
    ];
    let request = minimal_request(history);
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "c2lnbmF0dXJl");
    assert!(contents[2]["parts"][0].get("functionResponse").is_some());
}

#[test]
fn tools_use_json_schema_for_gemini_and_legacy_parameters_for_claude() {
    let mut request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    request.tools = vec![ToolDefinition {
        name: "bash".to_string(),
        description: "run shell".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string", "format": "shell"}},
            "required": ["command"],
            "$defs": {"x": {"type": "string"}}
        }),
    }];

    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let declaration = &body["request"]["tools"][0]["functionDeclarations"][0];
    assert!(declaration["parametersJsonSchema"].is_object());
    assert!(declaration["parametersJsonSchema"].get("$defs").is_none());
    assert!(declaration["parametersJsonSchema"]["properties"]["command"].is_object());

    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let declaration = &body["request"]["tools"][0]["functionDeclarations"][0];
    assert!(declaration["parameters"].is_object());
    // `format` is outside the protobuf allowlist and must be stripped.
    assert!(
        declaration["parameters"]["properties"]["command"]
            .get("format")
            .is_none()
    );
    assert_eq!(declaration["parameters"]["required"][0], "command");
    assert!(body["request"]["toolConfig"]["functionCallingConfig"]["mode"] == "VALIDATED");
}
