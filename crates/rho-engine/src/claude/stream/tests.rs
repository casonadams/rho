use super::*;
use rig::streaming::RawStreamingChoice;

#[test]
fn test_text_streaming_and_final_response() {
    let mut parser = SseParser::new();
    let stream_data = "\
event: message_start\n\
data: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 10}}}\n\n\
event: content_block_start\n\
data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\n\
event: content_block_delta\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Hello, \"}}\n\n\
event: content_block_delta\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"world!\"}}\n\n\
event: content_block_stop\n\
data: {\"type\": \"content_block_stop\", \"index\": 0}\n\n\
event: message_delta\n\
data: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 5}}\n\n\
event: message_stop\n\
data: {\"type\": \"message_stop\"}\n\n";

    let events = parser.feed(stream_data.as_bytes());
    assert_eq!(events.len(), 3);

    assert!(matches!(&events[0], Ok(RawStreamingChoice::Message(t)) if t == "Hello, "));
    assert!(matches!(&events[1], Ok(RawStreamingChoice::Message(t)) if t == "world!"));
    match &events[2] {
        Ok(RawStreamingChoice::FinalResponse(resp)) => {
            assert_eq!(resp.usage.input_tokens, 10);
            assert_eq!(resp.usage.output_tokens, 5);
            assert_eq!(resp.usage.total_tokens, 15);
            assert_eq!(resp.finish_reason, Some(FinishReason::Stop));
        }
        other => panic!("expected FinalResponse, got {other:?}"),
    }
}

#[test]
fn test_multibyte_utf8_split_across_chunks() {
    let mut parser = SseParser::new();
    // 🚀 is 4 bytes: F0 9F 9A 80
    // Split right inside a multibyte sequence
    let rocket_part1 = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"\xF0\x9F";
    let rocket_part2 = b"\x9A\x80\"}}\n";

    let events1 = parser.feed(rocket_part1);
    assert!(events1.is_empty());

    let events2 = parser.feed(rocket_part2);
    assert_eq!(events2.len(), 1);
    assert!(matches!(&events2[0], Ok(RawStreamingChoice::Message(t)) if t == "🚀"));
}

#[test]
fn test_chunk_boundary_split_inside_json() {
    let mut parser = SseParser::new();
    let chunk1 = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"ty";
    let chunk2 = b"pe\": \"text_delta\", \"text\": \"chunked\"}}\n";

    let events1 = parser.feed(chunk1);
    assert!(events1.is_empty());

    let events2 = parser.feed(chunk2);
    assert_eq!(events2.len(), 1);
    assert!(matches!(&events2[0], Ok(RawStreamingChoice::Message(t)) if t == "chunked"));
}

#[test]
fn test_thinking_and_signature_deltas() {
    let mut parser = SseParser::new();
    let payload = "\
data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"thinking\"}}\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"thinking_delta\", \"thinking\": \"step 1\"}}\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"signature_delta\", \"signature\": \"sig123\"}}\n\
data: {\"type\": \"content_block_stop\", \"index\": 0}\n";

    let events = parser.feed(payload.as_bytes());
    assert_eq!(events.len(), 3);

    assert!(matches!(&events[0], Ok(RawStreamingChoice::ReasoningStart { .. })));
    assert!(matches!(&events[1], Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) if reasoning == "step 1"));
    match &events[2] {
        Ok(RawStreamingChoice::ReasoningEnd {
            signature, reasoning, ..
        }) => {
            assert_eq!(signature.as_deref(), Some("sig123"));
            let r = reasoning.as_ref().unwrap();
            match &r.content[0] {
                rig::message::ReasoningContent::Text { text, signature } => {
                    assert_eq!(text, "step 1");
                    assert_eq!(signature.as_deref(), Some("sig123"));
                }
                _ => panic!("unexpected reasoning content"),
            }
        }
        other => panic!("expected ReasoningEnd, got {other:?}"),
    }
}

#[test]
fn test_mixed_tool_use_and_text() {
    let mut parser = SseParser::new();
    let payload = "\
data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\
data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Running tool:\"}}\n\
data: {\"type\": \"content_block_stop\", \"index\": 0}\n\
data: {\"type\": \"content_block_start\", \"index\": 1, \"content_block\": {\"type\": \"tool_use\", \"id\": \"tool_1\", \"name\": \"bash\"}}\n\
data: {\"type\": \"content_block_delta\", \"index\": 1, \"delta\": {\"type\": \"input_json_delta\", \"partial_json\": \"{\\\"cmd\\\": \"}}\n\
data: {\"type\": \"content_block_delta\", \"index\": 1, \"delta\": {\"type\": \"input_json_delta\", \"partial_json\": \"\\\"ls\\\"}\"}}\n\
data: {\"type\": \"content_block_stop\", \"index\": 1}\n";

    let events = parser.feed(payload.as_bytes());
    assert_eq!(events.len(), 2);

    assert!(matches!(&events[0], Ok(RawStreamingChoice::Message(t)) if t == "Running tool:"));
    match &events[1] {
        Ok(RawStreamingChoice::ToolCall(call)) => {
            assert_eq!(call.name, "bash");
            assert_eq!(call.arguments, serde_json::json!({ "cmd": "ls" }));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn test_stream_error_event() {
    let mut parser = SseParser::new();
    let payload = "data: {\"type\": \"error\", \"error\": {\"message\": \"rate limit exceeded\"}}\n";
    let events = parser.feed(payload.as_bytes());
    assert_eq!(events.len(), 1);
    match &events[0] {
        Err(CompletionError::ProviderError(msg)) => assert!(msg.contains("rate limit exceeded")),
        other => panic!("expected ProviderError, got {other:?}"),
    }
}
