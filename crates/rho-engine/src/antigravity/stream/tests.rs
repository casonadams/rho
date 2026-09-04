use super::SseParser;
use rig::completion::FinishReason;
use rig::message::ReasoningContent;
use rig::streaming::RawStreamingChoice;

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
                ReasoningContent::Text { text, .. } if text == "thinking...more"
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
