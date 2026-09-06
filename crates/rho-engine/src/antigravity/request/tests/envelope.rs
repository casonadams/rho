use super::*;
use rig::completion::ToolDefinition;
use rig::message::UserContent;

#[test]
fn request_envelope_has_project_model_and_agent_shape() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hello")],
    }]);
    let body = build_request_body(target("proj-1", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let actual = (
        body["project"].as_str(),
        body["model"].as_str(),
        body["requestType"].as_str(),
        body["userAgent"].as_str(),
    );
    assert_eq!(
        actual,
        (
            Some("proj-1"),
            Some("gemini-3.8-flash-low"),
            Some("agent"),
            Some("antigravity")
        )
    );
    assert_eq!(body["request"]["systemInstruction"]["role"], "user");
    assert_eq!(body["request"]["contents"][0]["parts"][0]["text"], "hello");
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );
}

fn tool_history_messages(
    prompt: &'static str,
    call: rig::message::ToolCall,
    result: rig::message::ToolResult,
) -> Vec<Message> {
    vec![
        Message::User {
            content: vec![UserContent::text(prompt)],
        },
        Message::Assistant {
            id: None,
            content: vec![rig::message::AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(result)],
        },
    ]
}

fn sample_tool_history(sig: Option<String>) -> Vec<Message> {
    let call_id = rig::message::ToolCallId::new("call-1").unwrap();
    let tool_call = rig::message::ToolCall {
        id: call_id.clone(),
        provider: None,
        function: rig::message::ToolFunction {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        },
        signature: sig,
        additional_params: None,
    };
    let tool_result = rig::message::ToolResult {
        call: call_id,
        provider: None,
        name: "read_file".to_string(),
        content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(
            "file body",
        ))],
    };
    tool_history_messages("read it", tool_call, tool_result)
}

#[test]
fn unsigned_tool_calls_flatten_to_observations_on_gemini_3() {
    let request = minimal_request(sample_tool_history(None));
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();

    assert!(!body.to_string().contains("functionCall"));
    let obs = contents[0]["parts"][1]["text"].as_str().unwrap();
    assert!(obs.contains("[Observation from `read_file`") && obs.contains("file body"));

    let body_claude = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let contents_claude = body_claude["request"]["contents"].as_array().unwrap();
    assert_eq!(contents_claude[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(contents_claude[2]["parts"][0]["functionResponse"]["name"], "read_file");
}

#[test]
fn signed_tool_calls_replay_function_calls_on_gemini_3() {
    let request = minimal_request(sample_tool_history(Some("c2lnbmF0dXJl".to_string())));
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "c2lnbmF0dXJl");
    assert!(contents[2]["parts"][0].get("functionResponse").is_some());
}

fn sample_bash_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "bash".to_string(),
        description: "run shell".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string", "format": "shell"}},
            "required": ["command"],
            "$defs": {"x": {"type": "string"}}
        }),
    }
}

#[test]
fn tools_use_json_schema_for_gemini() {
    let mut request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    request.tools = vec![sample_bash_tool_def()];
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let decl = &body["request"]["tools"][0]["functionDeclarations"][0];
    assert!(decl["parametersJsonSchema"].is_object() && decl["parametersJsonSchema"].get("$defs").is_none());
    assert!(decl["parametersJsonSchema"]["properties"]["command"].is_object());
}

#[test]
fn tools_use_legacy_parameters_for_claude() {
    let mut request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    request.tools = vec![sample_bash_tool_def()];
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let decl = &body["request"]["tools"][0]["functionDeclarations"][0];
    assert!(decl["parameters"].is_object() && decl["parameters"]["properties"]["command"].get("format").is_none());
    assert_eq!(decl["parameters"]["required"][0], "command");
    assert_eq!(
        body["request"]["toolConfig"]["functionCallingConfig"]["mode"],
        "VALIDATED"
    );
}

fn sample_image_tool_history() -> Vec<Message> {
    let call_id = rig::message::ToolCallId::new("call-1").unwrap();
    let tool_call = rig::message::ToolCall {
        id: call_id.clone(),
        provider: None,
        function: rig::message::ToolFunction {
            name: "read".to_string(),
            arguments: serde_json::json!({"path": "image.png"}),
        },
        signature: None,
        additional_params: None,
    };
    let tool_result = rig::message::ToolResult {
        call: call_id,
        provider: None,
        name: "read".to_string(),
        content: vec![
            rig::message::ToolResultContent::Text(rig::message::Text::new("Read image file")),
            rig::message::ToolResultContent::image_base64(
                "iVBORw0KGgo=",
                Some(rig::message::ImageMediaType::PNG),
                None,
            ),
        ],
    };
    tool_history_messages("read image", tool_call, tool_result)
}

#[test]
fn tool_result_with_image_claude_response() {
    let request = minimal_request(sample_image_tool_history());
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();
    let fn_resp = &contents[2]["parts"][0]["functionResponse"];
    assert_eq!(fn_resp["name"], "read");
    let parts = fn_resp["parts"].as_array().unwrap();
    assert_eq!(
        (
            parts.len(),
            parts[0]["inlineData"]["mimeType"].as_str(),
            parts[0]["inlineData"]["data"].as_str()
        ),
        (1, Some("image/png"), Some("iVBORw0KGgo="))
    );
}

#[test]
fn tool_result_with_image_gemini_observation() {
    let request = minimal_request(sample_image_tool_history());
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let parts = body["request"]["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(
        (
            parts[2]["inlineData"]["mimeType"].as_str(),
            parts[2]["inlineData"]["data"].as_str()
        ),
        (Some("image/png"), Some("iVBORw0KGgo="))
    );
}
