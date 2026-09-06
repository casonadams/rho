use super::SseParser;
use rig::completion::FinishReason;
use rig::message::ReasoningContent;
use rig::streaming::RawStreamingChoice;

fn assert_tool_call_event(event: &Result<RawStreamingChoice, rig::completion::CompletionError>) {
    if let Ok(RawStreamingChoice::ToolCall(call)) = event {
        assert_eq!(
            (call.name.as_str(), call.arguments["cmd"].as_str()),
            ("bash", Some("ls"))
        );
    } else {
        panic!("expected tool call");
    }
}

fn assert_final_event(event: &Result<RawStreamingChoice, rig::completion::CompletionError>) {
    if let Ok(RawStreamingChoice::FinalResponse(resp)) = event {
        assert_eq!(
            (
                resp.usage.input_tokens,
                resp.usage.output_tokens,
                resp.finish_reason.as_ref()
            ),
            (10, 5, Some(&FinishReason::Stop))
        );
    } else {
        panic!("expected final response");
    }
}

fn is_msg(event: &Result<RawStreamingChoice, rig::completion::CompletionError>, expected: &str) -> bool {
    matches!(event, Ok(RawStreamingChoice::Message(t)) if t == expected)
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
    assert!(is_msg(&events[0], "Hel") && is_msg(&events[1], "lo"));
    assert_tool_call_event(&events[2]);
    assert_final_event(&events[3]);
}

fn assert_reasoning_end_block(event: &Result<RawStreamingChoice, rig::completion::CompletionError>) {
    let Ok(RawStreamingChoice::ReasoningEnd {
        reasoning, signature, ..
    }) = event
    else {
        panic!()
    };
    assert_eq!(signature.as_deref(), Some("c2ln"));
    let block = reasoning.as_ref().unwrap();
    assert!(matches!(&block.content[0], ReasoningContent::Text { text, .. } if text == "thinking...more"));
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
    assert_reasoning_end_block(&events[3]);
    assert!(is_msg(&events[4], "answer"));
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
