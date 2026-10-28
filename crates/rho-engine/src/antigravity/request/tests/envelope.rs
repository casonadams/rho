use super::*;
use crate::provider::adapter::ModelToolDefinition;
use rho_harness_core::model::{AssistantContent, ChatMessage, Reasoning, ToolCall, ToolFunction, UserContent};

#[test]
fn request_envelope_has_project_model_and_agent_shape() {
    let request = minimal_request(vec![ChatMessage::user("hello")]);
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

fn tool_history_messages(prompt: &'static str, call: ToolCall, result_text: &'static str) -> Vec<ChatMessage> {
    vec![
        ChatMessage::user(prompt),
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call.clone())],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                call.id,
                call.function.name,
                result_text,
            ))],
        },
    ]
}

fn sample_tool_history() -> Vec<ChatMessage> {
    let tool_call = ToolCall::new(
        "call-1",
        ToolFunction::new("read_file", serde_json::json!({"path": "a.rs"})),
    );
    tool_history_messages("read it", tool_call, "file body")
}

#[test]
fn unsigned_tool_calls_bypass_sentinel_on_gemini_3() {
    let request = minimal_request(sample_tool_history());
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();

    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(
        contents[1]["parts"][0]["thoughtSignature"],
        "skip_thought_signature_validator"
    );
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "read_file");

    let body_claude = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let contents_claude = body_claude["request"]["contents"].as_array().unwrap();
    assert_eq!(contents_claude[1]["parts"][0]["functionCall"]["name"], "read_file");
    assert_eq!(contents_claude[2]["parts"][0]["functionResponse"]["name"], "read_file");
}

#[test]
fn parallel_tool_calls_omit_signature_on_secondary_calls() {
    let tool_call_1 = ToolCall::new(
        "call_1",
        ToolFunction::new("read_file", serde_json::json!({"path": "a.rs"})),
    );
    let tool_call_2 = ToolCall::new(
        "call_2",
        ToolFunction::new("grep", serde_json::json!({"pattern": "fn"})),
    );

    let messages = vec![
        ChatMessage::user("do search"),
        ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::ToolCall(tool_call_1),
                AssistantContent::ToolCall(tool_call_2),
            ],
        },
        ChatMessage::User {
            content: vec![
                UserContent::ToolResult(rho_harness_core::model::ToolResult::new("call_1", "read_file", "code")),
                UserContent::ToolResult(rho_harness_core::model::ToolResult::new("call_2", "grep", "match")),
            ],
        },
    ];

    let request = minimal_request(messages);
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();

    assert_eq!(
        contents[1]["parts"][0]["thoughtSignature"],
        "skip_thought_signature_validator"
    );
    assert!(contents[1]["parts"][1].get("thoughtSignature").is_none());
}

fn sample_image_tool_history() -> Vec<ChatMessage> {
    let tool_call = ToolCall::new(
        "call-1",
        ToolFunction::new("read", serde_json::json!({"path": "image.png"})),
    );
    let mut tr = rho_harness_core::model::ToolResult::new("call-1", "read", "Read image file");
    tr.content.push(rho_harness_core::model::ToolResultContent::image(
        "iVBORw0KGgo=",
        Some("image/png".to_string()),
    ));
    vec![
        ChatMessage::user("read image"),
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(tool_call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(tr)],
        },
    ]
}

#[test]
fn tool_result_with_image_multimodal_response() {
    for model in ["claude-sonnet-4-6", "gemini-3.8-flash-low"] {
        let request = minimal_request(sample_image_tool_history());
        let body = build_request_body(target("p", model), &request, &envelope()).unwrap();
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
}

#[test]
fn antigravity_tools_are_sorted_by_name() {
    let mut request = minimal_request(vec![ChatMessage::user("hi")]);
    request.tools = vec![
        ModelToolDefinition {
            name: "write".to_string(),
            description: "write file".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        },
        ModelToolDefinition {
            name: "bash".to_string(),
            description: "run shell".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        },
        ModelToolDefinition {
            name: "read".to_string(),
            description: "read file".to_string(),
            parameters: serde_json::json!({ "type": "object" }),
        },
    ];
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    let decls = body["request"]["tools"][0]["functionDeclarations"].as_array().unwrap();
    assert_eq!(decls.len(), 3);
    assert_eq!(decls[0]["name"], "bash");
    assert_eq!(decls[1]["name"], "read");
    assert_eq!(decls[2]["name"], "write");
}

#[test]
fn assistant_content_reasoning_converts_to_thought_parts() {
    let request = minimal_request(vec![
        ChatMessage::user("hello"),
        ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::Reasoning(Reasoning::new_with_signature(
                    "evaluating options",
                    Some("sig-xyz".to_string()),
                )),
                AssistantContent::text("final answer"),
            ],
        },
    ]);
    let body = build_request_body(target("p", "gemini-3.8-flash-low"), &request, &envelope()).unwrap();
    let contents = body["request"]["contents"].as_array().unwrap();
    let parts = contents[1]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["text"], "evaluating options");
    assert_eq!(parts[0]["thought"], true);
    assert_eq!(parts[0]["thoughtSignature"], "sig-xyz");
    assert_eq!(parts[1]["text"], "final answer");
}
