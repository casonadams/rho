use super::SseParser;
use crate::provider::adapter::ModelStreamEvent;
use rho_harness_core::error::AppError;

fn assert_tool_call_event(event: &Result<ModelStreamEvent, AppError>) {
    if let Ok(ModelStreamEvent::ToolCall(call)) = event {
        assert_eq!(call.function.name, "bash");
        assert!(call.function.arguments.to_string().contains("ls"));
    } else {
        panic!("expected tool call, got {event:?}");
    }
}

fn assert_final_event(event: &Result<ModelStreamEvent, AppError>) {
    if let Ok(ModelStreamEvent::Usage(usage)) = event {
        assert_eq!((usage.input_tokens, usage.output_tokens), (10, 5));
    } else {
        panic!("expected final response, got {event:?}");
    }
}

fn is_msg(event: &Result<ModelStreamEvent, AppError>, expected: &str) -> bool {
    matches!(event, Ok(ModelStreamEvent::Text(t)) if t == expected)
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
    assert_eq!(events.len(), 5);
    assert!(is_msg(&events[0], "Hel") && is_msg(&events[1], "lo"));
    assert_tool_call_event(&events[2]);
    assert_final_event(&events[3]);
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
    assert!(events[0].is_ok() && events[1].is_ok());
    assert!(is_msg(&events[2], "answer"));
}

#[test]
fn sse_parser_surfaces_in_band_error_chunks() {
    let mut parser = SseParser::new();
    let sse = "data: {\"error\":{\"code\":429,\"message\":\"Individual quota reached. Resets in 2h4m10s.\"}}\n\n";
    let events = parser.feed(sse.as_bytes());
    match &events[0] {
        Err(AppError::Provider(message)) => {
            assert!(message.contains("Individual quota reached"));
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[test]
fn sse_parser_attaches_thought_signature_to_tool_call() {
    let mut parser = SseParser::new();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"sig-123\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"command\":\"ls\"},\"id\":\"call-0\"}}]},\"finishReason\":\"STOP\"}]}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert_eq!(events.len(), 4);
    if let Ok(ModelStreamEvent::ToolCall(call)) = &events[1] {
        assert_eq!(call.signature.as_deref(), Some("sig-123"));
    } else {
        panic!("expected tool call with signature, got {:?}", events[1]);
    }
    assert!(matches!(&events[3], Ok(ModelStreamEvent::Finish(_))));
}

#[test]
fn sse_parser_does_not_leak_signature_to_secondary_tool_calls() {
    let mut parser = SseParser::new();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"sig-123\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"command\":\"ls\"},\"id\":\"call-0\"}},{\"functionCall\":{\"name\":\"grep\",\"args\":{\"pattern\":\"foo\"},\"id\":\"call-1\"}}]},\"finishReason\":\"STOP\"}]}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert_eq!(events.len(), 5);
    if let Ok(ModelStreamEvent::ToolCall(call)) = &events[1] {
        assert_eq!(call.function.name, "bash");
        assert_eq!(call.signature.as_deref(), Some("sig-123"));
    } else {
        panic!("expected tool call with signature, got {:?}", events[1]);
    }
    if let Ok(ModelStreamEvent::ToolCall(call)) = &events[2] {
        assert_eq!(call.function.name, "grep");
        assert_eq!(call.signature.as_deref(), None);
    } else {
        panic!("expected secondary tool call without signature, got {:?}", events[2]);
    }
    assert!(matches!(&events[4], Ok(ModelStreamEvent::Finish(_))));
}
