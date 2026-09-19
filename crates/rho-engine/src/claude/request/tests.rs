use super::*;
use rig::completion::ToolDefinition;
use rig::message::{
    AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use serde_json::json;

fn dummy_tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("Description for {name}"),
        parameters: json!({ "type": "object", "properties": {} }),
    }
}

fn sample_request_with_tools(tools: Vec<ToolDefinition>) -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("system preamble".to_string()),
        chat_history: vec![Message::user("hello")],
        documents: Vec::new(),
        tools,
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
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
fn custom_mcp_tools_are_sorted_alphabetically_by_name() {
    let tools = vec![
        dummy_tool("mcp__server__zebra"),
        dummy_tool("mcp__server__alpha"),
        dummy_tool("mcp__server__middle"),
    ];
    let req = sample_request_with_tools(tools);
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();
    let body_tools = body["tools"].as_array().expect("tools array present");

    assert_eq!(body_tools.len(), 3);
    assert_eq!(body_tools[0]["name"], "mcp__server__alpha");
    assert_eq!(body_tools[1]["name"], "mcp__server__middle");
    assert_eq!(body_tools[2]["name"], "mcp__server__zebra");
}

#[test]
fn single_turn_conversation_assigns_at_most_three_breakpoints() {
    let tools = vec![dummy_tool("read"), dummy_tool("bash")];
    let req = sample_request_with_tools(tools);
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    let total_breakpoints = count_cache_breakpoints(&body);
    assert!(total_breakpoints <= 3, "expected <= 3, got {total_breakpoints}");
    assert_eq!(total_breakpoints, 3);

    assert_eq!(body["system"][1]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][0]["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn multi_turn_conversation_with_three_turns_assigns_exactly_four_breakpoints() {
    let tools = vec![dummy_tool("bash")];
    let mut req = sample_request_with_tools(tools);
    req.chat_history = vec![
        Message::user("Turn 1 prompt"),
        Message::assistant("Turn 1 response"),
        Message::user("Turn 2 prompt"),
        Message::assistant("Turn 2 response"),
        Message::user("Turn 3 prompt"),
    ];
    let body = build_request_body("claude-sonnet-4-6", None, &req).unwrap();

    let total_breakpoints = count_cache_breakpoints(&body);
    assert_eq!(total_breakpoints, 4);

    assert_eq!(body["system"][1]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 5);

    assert!(messages[0]["content"][0].get("cache_control").is_none());
    assert!(messages[1]["content"][0].get("cache_control").is_none());
    assert!(messages[2]["content"][0].get("cache_control").is_none());

    assert_eq!(messages[3]["content"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(messages[4]["content"][0]["cache_control"]["type"], "ephemeral");
}

#[test]
fn multi_turn_with_in_flight_tool_results_maintains_prior_turn_checkpoint() {
    let tools = vec![dummy_tool("bash")];
    let mut req = sample_request_with_tools(tools);
    let call = ToolCall::new(
        ToolCallId::new("call_1").unwrap(),
        ToolFunction::new("bash".into(), json!({ "command": "pwd" })),
    );
    let res = ToolResult {
        call: ToolCallId::new("call_1").unwrap(),
        provider: None,
        name: "bash".into(),
        content: vec![ToolResultContent::Text(Text::new("/tmp"))],
    };
    req.chat_history = vec![
        Message::user("Turn 1 prompt"),
        Message::assistant("Turn 1 response"),
        Message::user("Turn 2 prompt"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(res)],
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
    req.chat_history = vec![
        Message::user("Turn 1 prompt"),
        Message::assistant("Turn 1 response"),
        Message::user("Turn 2 prompt"),
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
