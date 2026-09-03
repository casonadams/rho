//! Unit tests for the Antigravity wire format and runtime model mapping.

use super::request::*;
use super::stream::*;
use rig::completion::{CompletionRequest, FinishReason, ToolDefinition};
use rig::message::{Message, UserContent};
use rig::streaming::RawStreamingChoice;

fn minimal_request(history: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("system prompt".to_string()),
        chat_history: history,
        documents: Vec::new(),
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    }
}

fn envelope() -> Envelope {
    Envelope {
        request_id: "agent/test-id/123/traj/2".to_string(),
        session_id: "42".to_string(),
    }
}

fn target<'a>(project: &'static str, runtime_model: &'a str) -> RequestTarget<'a> {
    RequestTarget {
        project,
        runtime_model,
        effort: Effort::Off,
    }
}

fn high_target<'a>(project: &'static str, runtime_model: &'a str) -> RequestTarget<'a> {
    RequestTarget {
        project,
        runtime_model,
        effort: Effort::High,
    }
}

#[test]
fn runtime_mapping_covers_known_families_and_passes_through_unknown() {
    assert_eq!(
        resolve_runtime_model("gemini-3.8-flash", Effort::Off),
        "gemini-3.8-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.5-flash", Effort::Off),
        "gemini-3.5-flash-extra-low"
    );
    assert_eq!(
        resolve_runtime_model("claude-opus-4-6", Effort::Off),
        "claude-opus-4-6-thinking"
    );
    assert_eq!(
        resolve_runtime_model("gpt-oss-120b", Effort::Off),
        "gpt-oss-120b-medium"
    );
    // Runtime ids from the live catalog pass through untouched.
    assert_eq!(
        resolve_runtime_model("gemini-3.8-flash-high", Effort::High),
        "gemini-3.8-flash-high"
    );
    assert_eq!(
        resolve_runtime_model("claude-sonnet-4-6", Effort::High),
        "claude-sonnet-4-6"
    );
}

#[test]
fn fallback_chain_degrades_next_generation() {
    assert_eq!(
        fallback_runtime_model("gemini-3.8-flash-low"),
        Some("gemini-3.7-flash-low".to_string())
    );
    assert_eq!(
        fallback_runtime_model("gemini-3.7-flash-medium"),
        Some("gemini-3.6-flash-medium".to_string())
    );
    assert_eq!(fallback_runtime_model("gemini-3.6-flash-low"), None);
    assert_eq!(fallback_runtime_model("claude-sonnet-4-6"), None);
}

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
fn max_tokens_is_capped_per_runtime_family() {
    let mut request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    request.max_tokens = Some(1_000_000);
    let body = build_request_body(target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 64000);

    let body = build_request_body(target("p", "gpt-oss-120b-medium"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 32768);
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

#[test]
fn sse_parser_emits_text_tool_call_and_terminal() {
    let mut parser = SseParser::new();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"cmd\":\"ls\"},\"id\":\"t1\"}}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5,\"totalTokenCount\":15}}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0], Ok(RawStreamingChoice::Message(t)) if t == "Hel"));
    assert!(matches!(&events[1], Ok(RawStreamingChoice::Message(t)) if t == "lo"));
    match &events[2] {
        Ok(RawStreamingChoice::ToolCall(call)) => {
            assert_eq!(call.name, "bash");
            assert_eq!(call.arguments["cmd"], "ls");
        }
        other => panic!("expected tool call, got {other:?}"),
    }
    match &events[3] {
        Ok(RawStreamingChoice::FinalResponse(final_response)) => {
            assert_eq!(final_response.usage.input_tokens, 10);
            assert_eq!(final_response.usage.output_tokens, 5);
            assert_eq!(final_response.finish_reason, Some(FinishReason::Stop));
        }
        other => panic!("expected final response, got {other:?}"),
    }
}

#[test]
fn sse_parser_streams_thoughts_as_reasoning_blocks() {
    let mut parser = SseParser::new();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"thinking...\",\"thought\":true}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"more\",\"thought\":true,\"thoughtSignature\":\"c2ln\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"answer\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[]},\"finishReason\":\"STOP\"}]}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert!(matches!(events[0], Ok(RawStreamingChoice::ReasoningStart { .. })));
    assert!(matches!(
        &events[1],
        Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) if reasoning == "thinking..."
    ));
    assert!(matches!(&events[2], Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) if reasoning == "more"));
    match &events[3] {
        Ok(RawStreamingChoice::ReasoningEnd {
            reasoning, signature, ..
        }) => {
            let block = reasoning.as_ref().unwrap();
            assert!(matches!(
                &block.content[0],
                ReasoningContentForTest::Text { text, .. } if text == "thinking...more"
            ));
            assert_eq!(signature.as_deref(), Some("c2ln"));
        }
        other => panic!("expected reasoning end, got {other:?}"),
    }
    assert!(matches!(&events[4], Ok(RawStreamingChoice::Message(t)) if t == "answer"));
    assert!(matches!(events[5], Ok(RawStreamingChoice::FinalResponse(_))));
}

#[test]
fn sse_parser_surfaces_in_band_error_chunks() {
    let mut parser = SseParser::new();
    let sse = "data: {\"error\":{\"code\":429,\"message\":\"Individual quota reached. Resets in 2h4m10s.\"}}\n\n";
    let events = parser.feed(sse.as_bytes());
    match &events[0] {
        Err(rig::completion::CompletionError::ProviderError(message)) => {
            assert!(message.contains("Individual quota reached"));
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[test]
fn model_enum_label_uses_rollout_ids() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);
    let body = build_request_body(target("p", "gemini-3.5-flash-extra-low"), &request, &envelope()).unwrap();
    assert_eq!(body["request"]["labels"]["model_enum"], "MODEL_PLACEHOLDER_M187");
}

use rig::message::ReasoningContent as ReasoningContentForTest;

#[test]
fn thinking_level_routes_runtime_variants() {
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Off),
        "gemini-3.7-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Low),
        "gemini-3.7-flash-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::Medium),
        "gemini-3.7-flash-medium"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.7-flash", Effort::High),
        "gemini-3.7-flash-high"
    );
    // xhigh/max have no finer backend level; they ride high.
    assert_eq!(Effort::parse(Some("xhigh")), Effort::High);
    assert_eq!(Effort::parse(Some("max")), Effort::High);
    assert_eq!(Effort::parse(None), Effort::Off);
    // Agent aliases are the high variant of their family.
    assert_eq!(
        resolve_runtime_model("gemini-3.1-pro", Effort::High),
        "gemini-pro-agent"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.1-pro", Effort::Medium),
        "gemini-3.1-pro-low"
    );
    assert_eq!(
        resolve_runtime_model("gemini-3.5-flash", Effort::High),
        "gemini-3-flash-agent"
    );
}

#[test]
fn collapse_runtime_id_folds_tiers_into_families() {
    let (base, level) = collapse_runtime_id("gemini-3.7-flash-high");
    assert_eq!(base, "gemini-3.7-flash");
    assert_eq!(level, Some(Effort::High));

    let (base, level) = collapse_runtime_id("gemini-3.5-flash-extra-low");
    assert_eq!(base, "gemini-3.5-flash");
    assert_eq!(level, Some(Effort::Low));

    let (base, level) = collapse_runtime_id("gemini-3.6-flash-tiered");
    assert_eq!(base, "gemini-3.6-flash");
    assert_eq!(level, None);

    let (base, level) = collapse_runtime_id("gemini-3-flash-agent");
    assert_eq!(base, "gemini-3.5-flash");
    assert_eq!(level, Some(Effort::High));

    let (base, level) = collapse_runtime_id("claude-sonnet-4-6");
    assert_eq!(base, "claude-sonnet-4-6");
    assert_eq!(level, None);

    let (base, level) = collapse_runtime_id("gpt-oss-120b-medium");
    assert_eq!(base, "gpt-oss-120b");
    assert_eq!(level, Some(Effort::Medium));
}

#[test]
fn thinking_config_tracks_effort() {
    let request = minimal_request(vec![Message::User {
        content: vec![UserContent::text("hi")],
    }]);

    // Gemini flash: thinkingLevel follows the effort.
    let body = build_request_body(high_target("p", "gemini-3.7-flash-high"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "HIGH"
    );
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );

    // Off: includeThoughts false.
    let body = build_request_body(target("p", "gemini-3.7-flash-low"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );

    // 3.1-pro high routes to the agent id and uses a thinking budget.
    let body = build_request_body(high_target("p", "gemini-pro-agent"), &request, &envelope()).unwrap();
    assert_eq!(
        body["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        10001
    );

    // Claude takes no gemini thinkingConfig (beta header path instead).
    let body = build_request_body(high_target("p", "claude-sonnet-4-6"), &request, &envelope()).unwrap();
    assert!(body["request"]["generationConfig"].get("thinkingConfig").is_none());
    assert!(wants_claude_thinking_header("claude-sonnet-4-6", Effort::High));
    assert!(!wants_claude_thinking_header("claude-sonnet-4-6", Effort::Off));
    assert!(!wants_claude_thinking_header("gemini-3.7-flash-high", Effort::High));
}
