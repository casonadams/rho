use super::*;
use rig::streaming::RawStreamingChoice;

fn sample_stream_data() -> &'static str {
    "event: message_start\ndata: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 10}}}\n\nevent: content_block_start\ndata: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Hello, \"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"world!\"}}\n\nevent: content_block_stop\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\n\nevent: message_delta\ndata: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 5}}\n\nevent: message_stop\ndata: {\"type\": \"message_stop\"}\n\n"
}

fn is_stream_msg(choice: &Result<RawStreamingChoice, CompletionError>, expected: &str) -> bool {
    matches!(choice, Ok(RawStreamingChoice::Message(t)) if t == expected)
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
    if let Ok(RawStreamingChoice::FinalResponse(resp)) = &events[2] {
        assert_eq!(
            (
                resp.usage.input_tokens,
                resp.usage.output_tokens,
                resp.usage.total_tokens,
                resp.finish_reason.as_ref()
            ),
            (10, 5, 15, Some(&FinishReason::Stop))
        );
    }
}

#[test]
fn test_multibyte_utf8_split_across_chunks() {
    let mut parser = SseParser::new();
    let part1 = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"\xF0\x9F";
    assert!(parser.feed(part1).is_empty());
    let events2 = parser.feed(b"\x9A\x80\"}}\n");
    assert!(events2.len() == 1 && matches!(&events2[0], Ok(RawStreamingChoice::Message(t)) if t == "🚀"));
}

#[test]
fn test_chunk_boundary_split_inside_json() {
    let mut parser = SseParser::new();
    assert!(
        parser
            .feed(b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"ty")
            .is_empty()
    );
    let events2 = parser.feed(b"pe\": \"text_delta\", \"text\": \"chunked\"}}\n");
    assert!(events2.len() == 1 && matches!(&events2[0], Ok(RawStreamingChoice::Message(t)) if t == "chunked"));
}

#[test]
fn test_thinking_and_signature_deltas() {
    let mut parser = SseParser::new();
    let payload = "data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"thinking\"}}\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"thinking_delta\", \"thinking\": \"step 1\"}}\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"signature_delta\", \"signature\": \"sig123\"}}\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\n";
    let events = parser.feed(payload.as_bytes());
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], Ok(RawStreamingChoice::ReasoningStart { .. })));
    assert!(events[1].is_ok() && events[2].is_ok());
}

#[test]
fn test_mixed_tool_use_and_text() {
    let mut parser = SseParser::new();
    let payload = "data: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Running tool:\"}}\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\ndata: {\"type\": \"content_block_start\", \"index\": 1, \"content_block\": {\"type\": \"tool_use\", \"id\": \"tool_1\", \"name\": \"bash\"}}\ndata: {\"type\": \"content_block_delta\", \"index\": 1, \"delta\": {\"type\": \"input_json_delta\", \"partial_json\": \"{\\\"cmd\\\": \\\"ls\\\"}\"}}\ndata: {\"type\": \"content_block_stop\", \"index\": 1}\n";
    let events = parser.feed(payload.as_bytes());
    assert!(events.len() == 2 && is_stream_msg(&events[0], "Running tool:"));
    assert!(matches!(&events[1], Ok(RawStreamingChoice::ToolCall(_))));
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
