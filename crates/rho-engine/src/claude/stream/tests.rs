use super::*;
use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::FinishReason;
use crate::provider::adapter::ModelStreamEvent;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ToolCall, ToolFunction};

fn sample_stream_data() -> &'static str {
    "event: message_start\ndata: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 10}}}\n\nevent: content_block_start\ndata: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Hello, \"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"world!\"}}\n\nevent: content_block_stop\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\n\nevent: message_delta\ndata: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 5}}\n\nevent: message_stop\ndata: {\"type\": \"message_stop\"}\n\n"
}

fn is_stream_msg(choice: &Result<ModelStreamEvent, AppError>, expected: &str) -> bool {
    matches!(choice, Ok(ModelStreamEvent::Text(t)) if t == expected)
}

#[test]
fn test_text_streaming_chunks() {
    let mut parser = SseParser::new();
    let events = parser.feed(sample_stream_data().as_bytes());
    assert!(is_stream_msg(&events[0], "Hello, ") && is_stream_msg(&events[1], "world!"));
}

#[test]
fn test_text_streaming_final_response() {
    let mut parser = SseParser::new();
    let events = parser.feed(sample_stream_data().as_bytes());
    if let Ok(ModelStreamEvent::Usage(usage)) = &events[2] {
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (10, 5, 15)
        );
    }
}

#[test]
fn test_multibyte_utf8_split_across_chunks() {
    let mut parser = SseParser::new();
    let part1 = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"\xF0\x9F";
    assert!(parser.feed(part1).is_empty());
    let events2 = parser.feed(b"\x9A\x80\"}}\n");
    assert!(events2.len() == 1 && matches!(&events2[0], Ok(ModelStreamEvent::Text(t)) if t == "🚀"));
}

#[test]
fn test_chunk_boundary_split_inside_json() {
    let mut parser = SseParser::new();
    assert!(
        parser
            .feed(b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"ty")
            .is_empty()
    );
    let events = parser.feed(b"pe\": \"text_delta\", \"text\": \"split\"}}\n");
    assert!(is_stream_msg(&events[0], "split"));
}

type ParsedEvent = Result<ModelStreamEvent, String>;

fn parse_payloads(payloads: &[&str]) -> Vec<ParsedEvent> {
    let body: String = payloads.iter().map(|payload| format!("data: {payload}\n\n")).collect();
    SseParser::new()
        .feed(body.as_bytes())
        .into_iter()
        .map(|event| event.map_err(|error| error.to_string()))
        .collect()
}

#[test]
fn stream_payloads_decode_to_domain_events() {
    for (name, payloads, expected) in decode_cases() {
        assert_eq!(parse_payloads(&payloads), expected, "case: {name}");
    }
}

fn decode_cases() -> Vec<(&'static str, Vec<&'static str>, Vec<ParsedEvent>)> {
    vec![
        (
            "text delta",
            vec![r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#],
            vec![Ok(ModelStreamEvent::Text("Hi".into()))],
        ),
        (
            "thinking block emits reasoning and closes on stop",
            vec![
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
            ],
            vec![Ok(ModelStreamEvent::Reasoning("hmm".into()))],
        ),
        (
            "tool use input assembled across json deltas",
            vec![
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"Read"}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"a.rs\"}"}}"#,
                r#"{"type":"content_block_stop","index":1}"#,
            ],
            vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "toolu_1",
                ToolFunction::new("read", serde_json::json!({"path": "a.rs"})),
            )))],
        ),
        (
            "tool use with unparseable input keeps raw text and strips mcp prefix",
            vec![
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"mcp__rho__custom_tool"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"x\":"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
            ],
            vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "t1",
                ToolFunction::new("custom_tool", serde_json::json!({"raw": "{\"x\":"})),
            )))],
        ),
        (
            "input json for an unknown block index is dropped",
            vec![
                r#"{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
                r#"{"type":"content_block_stop","index":3}"#,
            ],
            vec![],
        ),
        (
            "unrecognised block, delta, and event types are ignored",
            vec![
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"citations_delta"}}"#,
                r#"{"type":"ping"}"#,
            ],
            vec![],
        ),
        (
            "usage accumulates from message start and message delta",
            vec![
                r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":3,"cache_creation_input_tokens":2,"output_tokens_details":{"thinking_tokens":4}}}}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7,"input_tokens":12}}"#,
                r#"{"type":"message_stop"}"#,
            ],
            vec![
                Ok(ModelStreamEvent::Usage(StructuralUsage {
                    input_tokens: 12,
                    output_tokens: 7,
                    total_tokens: 24,
                    cached_input_tokens: Some(3),
                    cache_creation_input_tokens: Some(2),
                    tool_use_prompt_tokens: None,
                    reasoning_tokens: Some(4),
                })),
                Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
            ],
        ),
        (
            "message delta without usage keeps start counts",
            vec![
                r#"{"type":"message_start","message":{"usage":{"input_tokens":5}}}"#,
                r#"{"type":"message_delta","delta":{}}"#,
                r#"{"type":"message_stop"}"#,
            ],
            vec![
                Ok(ModelStreamEvent::Usage(StructuralUsage {
                    input_tokens: 5,
                    total_tokens: 5,
                    ..StructuralUsage::default()
                })),
                Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
            ],
        ),
        (
            "message delta reports thinking tokens",
            vec![
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9,"output_tokens_details":{"thinking_tokens":6}}}"#,
                r#"{"type":"message_stop"}"#,
            ],
            vec![
                Ok(ModelStreamEvent::Usage(StructuralUsage {
                    output_tokens: 9,
                    total_tokens: 9,
                    reasoning_tokens: Some(6),
                    ..StructuralUsage::default()
                })),
                Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
            ],
        ),
        (
            "message stop without usage reports zero counts",
            vec![r#"{"type":"message_stop"}"#],
            vec![
                Ok(ModelStreamEvent::Usage(StructuralUsage::default())),
                Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
            ],
        ),
        (
            "error event surfaces the provider message",
            vec![r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#],
            vec![Err("Provider error: Overloaded".to_string())],
        ),
        (
            "error event without a message uses the default",
            vec![r#"{"type":"error","error":{"type":"api_error"}}"#],
            vec![Err("Provider error: Anthropic streaming error".to_string())],
        ),
    ]
}

#[test]
fn message_delta_stop_reason_maps_to_finish_reason() {
    let cases = [
        ("end_turn", FinishReason::Stop),
        ("stop_sequence", FinishReason::Stop),
        ("max_tokens", FinishReason::Length),
        ("tool_use", FinishReason::ToolCalls),
        ("unknown_reason", FinishReason::Other),
    ];
    for (stop_reason, expected) in cases {
        let delta = format!(r#"{{"type":"message_delta","delta":{{"stop_reason":"{stop_reason}"}}}}"#);
        let events = parse_payloads(&[&delta, r#"{"type":"message_stop"}"#]);
        assert_eq!(
            events.last(),
            Some(&Ok(ModelStreamEvent::Finish(expected))),
            "stop_reason: {stop_reason}"
        );
    }
}
