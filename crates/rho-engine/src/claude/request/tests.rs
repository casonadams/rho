use super::*;
use crate::claude::test_util::{login_body, message_marks};
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use rho_harness_core::model::{
    AssistantContent, ChatMessage, ImageContent, Reasoning, ReasoningContent, ToolCall, ToolFunction, UserContent,
};
use serde_json::json;
use test_case::test_case;

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
        messages: Default::default(),
        tools,
        max_tokens: None,
        temperature: None,
        thinking_level: None,
        conversation_id: None,
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
    let body = login_body("claude-sonnet-4-6", None, &req);
    let body_tools = body["tools"].as_array().expect("tools array present");

    assert_eq!(body_tools.len(), 4);
    assert_eq!(body_tools[0]["name"], "Bash");
    assert_eq!(body_tools[1]["name"], "Edit");
    assert_eq!(body_tools[2]["name"], "Read");
    assert_eq!(body_tools[3]["name"], "Write");
}

#[test]
fn only_the_write_and_edit_tools_stream_their_input_eagerly() {
    let req = sample_request_with_tools(vec![dummy_tool("write"), dummy_tool("bash"), dummy_tool("edit")]);
    let body = login_body("claude-sonnet-4-6", None, &req);
    let eager: Vec<(&str, bool)> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            let eager = tool["eager_input_streaming"].as_bool().unwrap_or(false);
            (tool["name"].as_str().unwrap(), eager)
        })
        .collect();
    assert_eq!(eager, [("Bash", false), ("Edit", true), ("Write", true)]);
}

#[test]
fn single_user_turn_no_cache_control_on_user_message() {
    let req = sample_request_with_tools(vec![dummy_tool("bash")]);
    assert_eq!(message_marks(&login_body("claude-sonnet-4-6", None, &req)), [false]);
}

#[test]
fn single_user_turn_cache_control_on_system_and_tools() {
    let req = sample_request_with_tools(vec![dummy_tool("bash")]);
    let body = login_body("claude-sonnet-4-6", None, &req);

    assert_eq!(
        body["system"].as_array().unwrap().last().unwrap()["cache_control"]["type"],
        "ephemeral"
    );
    assert_eq!(
        body["tools"].as_array().unwrap().last().unwrap()["cache_control"]["type"],
        "ephemeral"
    );
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
    ]
    .into();
    let body = login_body("claude-sonnet-4-6", None, &req);

    assert_eq!(count_cache_breakpoints(&body), 4);
    assert_eq!(message_marks(&body), [false, true, false, false, true]);
}

#[test]
fn empty_tools_assigns_three_breakpoints_in_multi_turn() {
    let mut req = sample_request_with_tools(Vec::new());
    req.prompt = String::new();
    req.messages = vec![
        ChatMessage::user("Turn 1 prompt"),
        ChatMessage::assistant("Turn 1 response"),
        ChatMessage::user("Turn 2 prompt"),
    ]
    .into();
    let body = login_body("claude-sonnet-4-6", None, &req);

    assert!(body.get("tools").is_none());
    assert_eq!(count_cache_breakpoints(&body), 3);
    assert_eq!(message_marks(&body), [false, true, true]);
}

#[test]
fn login_requests_lead_with_claude_code_identity() {
    let req = sample_request_with_tools(Vec::new());
    let body = login_body("claude-sonnet-4-6", None, &req);
    assert_eq!(
        body["system"][0]["text"],
        "You are Claude Code, Anthropic's official CLI for Claude."
    );
}

#[test]
fn the_system_prompt_names_each_renamed_tool() {
    let req = sample_request_with_tools(vec![
        dummy_tool("read"),
        dummy_tool("fd"),
        dummy_tool("mcp__github__get_me"),
    ]);
    let body = login_body("claude-sonnet-4-6", None, &req);
    assert_eq!(
        body["system"][1]["text"],
        "system preamble\n\nThese instructions name tools by their short names; call `fd` as `mcp__rho__fd` and `read` as `Read`."
    );

    let without_tools = login_body("claude-sonnet-4-6", None, &sample_request_with_tools(Vec::new()));
    assert_eq!(without_tools["system"][1]["text"], "system preamble");
}

#[test]
fn api_key_requests_omit_claude_code_identity() {
    let req = sample_request_with_tools(vec![dummy_tool("bash")]);
    let body = build_request_body(
        "claude-sonnet-4-6",
        None,
        &req,
        ClaudeAuth::ApiKey,
        CacheRetention::Short,
    )
    .unwrap();
    assert!(!body.to_string().contains("You are Claude Code"));
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
}

#[test_case(CacheRetention::Short, ClaudeAuth::Login, None; "short login")]
#[test_case(CacheRetention::Long, ClaudeAuth::ApiKey, Some("1h"); "long api key")]
#[test_case(CacheRetention::Auto, ClaudeAuth::Login, Some("1h"); "auto login")]
#[test_case(CacheRetention::Auto, ClaudeAuth::ApiKey, None; "auto api key")]
fn cache_retention_sets_the_ttl_of_every_breakpoint(retention: CacheRetention, auth: ClaudeAuth, ttl: Option<&str>) {
    let mut req = sample_request_with_tools(vec![dummy_tool("bash")]);
    req.prompt = String::new();
    req.messages = vec![
        ChatMessage::user("Turn 1 prompt"),
        ChatMessage::assistant("Turn 1 response"),
        ChatMessage::user("Turn 2 prompt"),
    ]
    .into();
    let body = build_request_body("claude-opus-5-5", None, &req, auth, retention).unwrap();
    assert_eq!(count_cache_breakpoints(&body), 4);
    let controls = [
        &body["system"].as_array().unwrap().last().unwrap()["cache_control"],
        &body["tools"][0]["cache_control"],
        &body["messages"][1]["content"][0]["cache_control"],
        &body["messages"][2]["content"][0]["cache_control"],
    ];
    for control in controls {
        assert_eq!(control["type"], "ephemeral");
        assert_eq!(control.get("ttl").and_then(Value::as_str), ttl);
    }
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
    mark_cache_breakpoints(&mut body, CacheRetention::Short);

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
        messages: vec![user_msg].into(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
        thinking_level: None,
        conversation_id: None,
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
fn a_compaction_summary_in_the_history_reaches_the_model_as_user_text() {
    let req = ModelCompletionRequest {
        messages: vec![
            ChatMessage::System {
                content: "Summary of earlier work".into(),
            },
            ChatMessage::user("continue"),
        ]
        .into(),
        ..ModelCompletionRequest::default()
    };

    let messages = contents::convert_messages(&req);

    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0]["role"], "user");
    let texts: Vec<&str> = messages[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect();
    assert_eq!(texts, ["Summary of earlier work", "continue"]);
}

#[test]
fn convert_messages_replays_signed_and_redacted_thinking_and_drops_unsigned() {
    let asst_msg = ChatMessage::Assistant {
        id: None,
        content: vec![
            AssistantContent::Reasoning(Reasoning::new("unsigned from another provider")),
            AssistantContent::Reasoning(Reasoning::new_with_signature(
                "  let me think\n",
                Some("sig_123".to_string()),
            )),
            AssistantContent::Reasoning(Reasoning::new_with_signature("", Some("sig_empty".to_string()))),
            AssistantContent::Reasoning(Reasoning {
                id: None,
                content: vec![ReasoningContent::Redacted { data: "enc".into() }],
            }),
            AssistantContent::text("done"),
        ],
    };

    let req = ModelCompletionRequest {
        prompt: String::new(),
        preamble: None,
        messages: vec![asst_msg].into(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
        thinking_level: None,
        conversation_id: None,
    };

    let messages = contents::convert_messages(&req);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "Hello");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(
        messages[1]["content"],
        json!([
            { "type": "thinking", "thinking": "  let me think\n", "signature": "sig_123" },
            { "type": "thinking", "thinking": "", "signature": "sig_empty" },
            { "type": "redacted_thinking", "data": "enc" },
            { "type": "text", "text": "done" },
        ])
    );
}

#[test]
fn cache_breakpoints_never_mark_thinking_blocks() {
    let mut body = json!({
        "messages": [
            { "role": "user", "content": [{ "type": "text", "text": "q" }] },
            { "role": "assistant", "content": [
                { "type": "text", "text": "a" },
                { "type": "thinking", "thinking": "t", "signature": "s" },
            ] },
            { "role": "user", "content": [{ "type": "text", "text": "next" }] },
        ]
    });

    mark_cache_breakpoints(&mut body, CacheRetention::Short);

    let assistant = &body["messages"][1]["content"];
    assert!(assistant[0].get("cache_control").is_some());
    assert!(assistant[1].get("cache_control").is_none());
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
        }]
        .into(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
        thinking_level: None,
        conversation_id: None,
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

#[test_case("mcp__rho__custom_tool", "custom_tool"; "mcp prefixed")]
#[test_case("Read", "read"; "read pascal case")]
#[test_case("READ", "read"; "read upper case")]
#[test_case("Write", "write"; "write")]
#[test_case("Edit", "edit"; "edit")]
#[test_case("Bash", "bash"; "bash")]
#[test_case("Glob", "glob"; "glob")]
#[test_case("Grep", "grep"; "grep")]
#[test_case("WebFetch", "web_fetch"; "web fetch pascal case")]
#[test_case("webfetch", "web_fetch"; "web fetch lower case")]
#[test_case("web_fetch", "web_fetch"; "web fetch snake case")]
#[test_case("WebSearch", "web_search"; "web search pascal case")]
#[test_case("websearch", "web_search"; "web search lower case")]
#[test_case("web_search", "web_search"; "web search snake case")]
#[test_case("unknown_tool", "unknown_tool"; "unknown tool")]
fn from_claude_tool_name_mapping(input: &str, expected: &str) {
    assert_eq!(from_claude_tool_name(input), expected);
}
