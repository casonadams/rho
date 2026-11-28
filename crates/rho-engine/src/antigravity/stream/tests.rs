use super::sse_parser;
use crate::provider::adapter::{FinishReason, ModelStreamEvent};
use rho_harness_core::error::AppError;
use test_case::test_case;

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
    let mut parser = sse_parser();
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
    let mut parser = sse_parser();
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
    let mut parser = sse_parser();
    let sse = "data: {\"error\":{\"code\":429,\"message\":\"Individual quota reached. Resets in 2h4m10s.\"}}\n\n";
    let events = parser.feed(sse.as_bytes());
    match &events[0] {
        Err(AppError::Provider(message)) => {
            assert!(message.contains("Individual quota reached"));
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

fn feed_payloads(payloads: &[&str]) -> Vec<Result<ModelStreamEvent, AppError>> {
    let body: String = payloads.iter().map(|p| format!("data: {p}\n\n")).collect();
    sse_parser().feed(body.as_bytes())
}

#[test_case("STOP", FinishReason::Stop; "stop")]
#[test_case("MAX_TOKENS", FinishReason::Length; "max tokens")]
#[test_case("SAFETY", FinishReason::ContentFilter; "safety")]
fn finish_reasons_map_to_their_canonical_reason(reason: &str, expected: FinishReason) {
    let line = format!(r#"{{"response":{{"candidates":[{{"finishReason":"{reason}"}}]}}}}"#);
    let events = feed_payloads(&[&line]);
    assert!(
        matches!(events.last(), Some(Ok(ModelStreamEvent::Finish(r))) if *r == expected),
        "{events:?}"
    );
}

#[test]
fn usage_from_an_earlier_chunk_is_kept_when_the_finishing_chunk_has_none() {
    let events = feed_payloads(&[
        r#"{"response":{"candidates":[{"content":{"parts":[{"text":"hi"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5,"totalTokenCount":15}}}"#,
        r#"{"response":{"candidates":[{"finishReason":"STOP"}]}}"#,
    ]);
    let usages: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Ok(ModelStreamEvent::Usage(u)) => Some(u.total_tokens),
            _ => None,
        })
        .collect();
    assert_eq!(usages, vec![15], "{events:?}");
}

#[tokio::test]
async fn stream_closed_before_finish_reason_is_a_network_error() {
    let payload = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"cut off"}]}}]}}"#;
    let events = crate::provider::sse::collect_sse_payloads(&[payload], sse_parser()).await;
    assert!(is_msg(&events[0], "cut off"));
    assert!(matches!(events.last(), Some(Err(AppError::Network(_)))), "{events:?}");
}

#[tokio::test]
async fn stream_closed_after_finish_reason_is_complete() {
    let payload = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}}"#;
    let events = crate::provider::sse::collect_sse_payloads(&[payload], sse_parser()).await;
    assert!(events.iter().all(Result::is_ok), "{events:?}");
    assert!(matches!(
        events.last(),
        Some(Ok(ModelStreamEvent::Finish(FinishReason::Stop)))
    ));
}

#[test]
fn sse_parser_attaches_thought_signature_to_tool_call() {
    let mut parser = sse_parser();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"sig-123\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"command\":\"ls\"},\"id\":\"call-0\"}}]},\"finishReason\":\"STOP\"}]}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert_eq!(events.len(), 3);
    if let Ok(ModelStreamEvent::ToolCall(call)) = &events[1] {
        assert_eq!(call.signature.as_deref(), Some("sig-123"));
    } else {
        panic!("expected tool call with signature, got {:?}", events[1]);
    }
    assert!(matches!(&events[2], Ok(ModelStreamEvent::Finish(_))));
}

#[test]
fn sse_parser_does_not_leak_signature_to_secondary_tool_calls() {
    let mut parser = sse_parser();
    let sse = concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"plan\",\"thought\":true,\"thoughtSignature\":\"sig-123\"}]}}]}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"command\":\"ls\"},\"id\":\"call-0\"}},{\"functionCall\":{\"name\":\"grep\",\"args\":{\"pattern\":\"foo\"},\"id\":\"call-1\"}}]},\"finishReason\":\"STOP\"}]}}\n\n"
    );
    let events = parser.feed(sse.as_bytes());
    assert_eq!(events.len(), 4);
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
    assert!(matches!(&events[3], Ok(ModelStreamEvent::Finish(_))));
}
