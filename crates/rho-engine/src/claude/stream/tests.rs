use super::*;

fn sample_stream_data() -> &'static str {
    "event: message_start\ndata: {\"type\": \"message_start\", \"message\": {\"id\": \"msg_1\", \"usage\": {\"input_tokens\": 10}}}\n\nevent: content_block_start\ndata: {\"type\": \"content_block_start\", \"index\": 0, \"content_block\": {\"type\": \"text\"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"Hello, \"}}\n\nevent: content_block_delta\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"world!\"}}\n\nevent: content_block_stop\ndata: {\"type\": \"content_block_stop\", \"index\": 0}\n\nevent: message_delta\ndata: {\"type\": \"message_delta\", \"delta\": {\"stop_reason\": \"end_turn\"}, \"usage\": {\"output_tokens\": 5}}\n\nevent: message_stop\ndata: {\"type\": \"message_stop\"}\n\n"
}

fn is_stream_msg(choice: &Result<AdapterFrame, ProviderError>, expected: &str) -> bool {
    matches!(choice, Ok(AdapterFrame::Text(t)) if t == expected)
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
    if let Ok(AdapterFrame::Done { usage }) = &events[2] {
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (Some(10), Some(5), Some(15))
        );
    }
}

#[test]
fn test_multibyte_utf8_split_across_chunks() {
    let mut parser = SseParser::new();
    let part1 = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"\xF0\x9F";
    assert!(parser.feed(part1).is_empty());
    let events2 = parser.feed(b"\x9A\x80\"}}\n");
    assert!(events2.len() == 1 && matches!(&events2[0], Ok(AdapterFrame::Text(t)) if t == "🚀"));
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

#[test]
fn test_mixed_newline_framing_preserves_payload() {
    let mut parser = SseParser::new();
    let raw = b"data: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"line1\"}}\r\ndata: {\"type\": \"content_block_delta\", \"index\": 0, \"delta\": {\"type\": \"text_delta\", \"text\": \"line2\"}}\n";
    let events = parser.feed(raw);
    assert!(is_stream_msg(&events[0], "line1") && is_stream_msg(&events[1], "line2"));
}
