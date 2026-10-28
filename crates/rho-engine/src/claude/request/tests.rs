use super::*;
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, Reasoning, ToolCall, ToolFunction, UserContent,
};
use serde_json::json;

fn dummy_tool(name: &str) -> ModelToolDefinition {
    ModelToolDefinition {
        name: name.to_string(),
        description: format!("Description for {name}"),
        parameters: json!({ "type": "object", "properties": {} }),
    }
}

fn sample_request_with_tools(tools: Vec<ModelToolDefinition>) -> ModelCompletionRequest {
    ModelCompletionRequest {
        preamble: Some("system preamble".to_string()),
        prompt: "hello".to_string(),
        messages: Vec::new(),
        tools,
        max_tokens: None,
        temperature: None,
    }
}

#[test]
fn tools_are_sorted_alphabetically_by_name() {
    let tools = vec![
        dummy_tool("write"),
        dummy_tool("bash"),
        dummy_tool("read"),
        dummy_tool("edit"),
    ];
    let req = sample_request_with_tools(tools);
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();
    let body_tools = body["tools"].as_array().expect("tools array present");

    assert_eq!(body_tools.len(), 4);
    assert_eq!(body_tools[0]["name"], "Bash");
    assert_eq!(body_tools[1]["name"], "Edit");
    assert_eq!(body_tools[2]["name"], "Read");
    assert_eq!(body_tools[3]["name"], "Write");
}

#[test]
fn single_user_turn_no_cache_control_on_user_message() {
    let req = sample_request_with_tools(vec![dummy_tool("bash")]);
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0]["content"][0].get("cache_control").is_none());
}

#[test]
fn single_user_turn_cache_control_on_system_and_tools() {
    let req = sample_request_with_tools(vec![dummy_tool("bash")]);
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    let system = body["system"].as_array().unwrap();
    assert_eq!(system.last().unwrap()["cache_control"]["type"], "ephemeral");

    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.last().unwrap()["cache_control"]["type"], "ephemeral");
}

#[test]
fn cache_breakpoints_capped_at_four_in_deep_conversation() {
    let mut req = sample_request_with_tools(vec![dummy_tool("bash")]);
    req.prompt = String::new();
    let call = ToolCall::new("call_1", ToolFunction::new("bash", json!({ "command": "pwd" })));
    req.messages = vec![
        ChatMessage::user("Turn 1 prompt"),
        ChatMessage::assistant("Turn 1 response"),
        ChatMessage::user("Turn 2 prompt"),
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                "call_1", "bash", "/tmp",
            ))],
        },
    ];
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    let total = count_cache_breakpoints(&body);
    assert_eq!(total, 4);

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 5);

    assert_eq!(messages[1]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(messages[4]["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn empty_tools_assigns_three_breakpoints_in_multi_turn() {
    let mut req = sample_request_with_tools(Vec::new());
    req.prompt = String::new();
    req.messages = vec![
        ChatMessage::user("Turn 1 prompt"),
        ChatMessage::assistant("Turn 1 response"),
        ChatMessage::user("Turn 2 prompt"),
    ];
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    assert!(body.get("tools").is_none());
    let total = count_cache_breakpoints(&body);
    assert_eq!(total, 3);

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[1]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(messages[2]["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn empty_system_blocks_assigns_breakpoints_without_exceeding_four() {
    let mut body = json!({
        "system": [],
        "tools": [
            { "name": "bash", "description": "run command" }
        ],
        "messages": [
            { "role": "user", "content": [{ "type": "text", "text": "turn 1" }] },
            { "role": "assistant", "content": [{ "type": "text", "text": "resp 1" }] },
            { "role": "user", "content": [{ "type": "text", "text": "turn 2" }] }
        ]
    });
    mark_cache_breakpoints(&mut body);

    let total = count_cache_breakpoints(&body);
    assert_eq!(total, 3);
    assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][1]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][2]["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn convert_messages_converts_user_and_assistant_images() {
    let user_msg = ChatMessage::User {
        content: vec![
            UserContent::text("Look:"),
            UserContent::Image(ImageContent {
                data: "b64jpeg".to_string(),
                media_type: Some("image/jpeg".to_string()),
            }),
            UserContent::Image(ImageContent {
                data: "rawpng".to_string(),
                media_type: Some("image/png".to_string()),
            }),
            UserContent::Image(ImageContent {
                data: "b64gif".to_string(),
                media_type: Some("image/gif".to_string()),
            }),
            UserContent::Image(ImageContent {
                data: "b64webp".to_string(),
                media_type: Some("image/webp".to_string()),
            }),
        ],
    };

    let req = ModelCompletionRequest {
        prompt: String::new(),
        preamble: None,
        messages: vec![user_msg],
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
    };

    let messages = contents::convert_messages(&req);
    assert_eq!(messages.len(), 1);
    let parts = messages[0]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["source"]["media_type"], "image/jpeg");
    assert_eq!(parts[1]["source"]["data"], "b64jpeg");
    assert_eq!(parts[2]["source"]["media_type"], "image/png");
    assert_eq!(parts[3]["source"]["media_type"], "image/gif");
    assert_eq!(parts[4]["source"]["media_type"], "image/webp");
}

#[test]
fn convert_messages_handles_reasoning_and_assistant_first() {
    let asst_msg = ChatMessage::Assistant {
        id: None,
        content: vec![
            AssistantContent::Reasoning(Reasoning::new_with_signature(
                "let me think",
                Some("sig_123".to_string()),
            )),
            AssistantContent::text("done"),
        ],
    };

    let req = ModelCompletionRequest {
        prompt: String::new(),
        preamble: None,
        messages: vec![asst_msg],
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
    };

    let messages = contents::convert_messages(&req);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "Hello");
    assert_eq!(messages[1]["role"], "assistant");
    let assistant_parts = messages[1]["content"].as_array().unwrap();
    assert_eq!(assistant_parts.len(), 2);
    assert_eq!(assistant_parts[0]["type"], "thinking");
    assert_eq!(assistant_parts[0]["signature"], "sig_123");
    assert_eq!(assistant_parts[1]["type"], "text");
}

#[test]
fn convert_messages_handles_tool_results() {
    let req = ModelCompletionRequest {
        prompt: String::new(),
        preamble: None,
        messages: vec![ChatMessage::User {
            content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                "call_json",
                "bash",
                "result text",
            ))],
        }],
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
    };

    let messages = contents::convert_messages(&req);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content"][0]["type"], "tool_result");
    assert_eq!(messages[0]["content"][0]["content"], "result text");
}

#[test]
fn calculate_max_tokens_adaptive_cases() {
    assert_eq!(calculate_max_tokens(None, None, true, Some("max")), 32768);
    assert_eq!(calculate_max_tokens(None, None, true, Some("xhigh")), 32768);
    assert_eq!(calculate_max_tokens(None, None, true, Some("low")), 16384);
    assert_eq!(calculate_max_tokens(None, Some(10000), false, None), 14096);
    assert_eq!(calculate_max_tokens(Some(5000), Some(6000), false, None), 7024);
}

#[test]
fn from_claude_tool_name_mapping() {
    let cases = [
        ("mcp__rho__custom_tool", "custom_tool"),
        ("Read", "read"),
        ("READ", "read"),
        ("Write", "write"),
        ("Edit", "edit"),
        ("Bash", "bash"),
        ("Glob", "glob"),
        ("Grep", "grep"),
        ("WebFetch", "web_fetch"),
        ("webfetch", "web_fetch"),
        ("web_fetch", "web_fetch"),
        ("WebSearch", "web_search"),
        ("websearch", "web_search"),
        ("web_search", "web_search"),
        ("unknown_tool", "unknown_tool"),
    ];
    for (input, expected) in cases {
        assert_eq!(from_claude_tool_name(input), expected);
    }
}
