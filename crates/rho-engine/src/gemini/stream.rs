//! Native Google Generative Language API SSE stream parser.
//!
//! Streams thinking deltas live and flushes `ModelStreamEvent::ReasoningBlock` with
//! `thoughtSignature` so multi-turn Gemini sessions preserve reasoning continuity.

use crate::provider::google_wire::{GoogleSseParser, ThoughtReplay};

pub fn sse_parser() -> GoogleSseParser {
    GoogleSseParser::new(ThoughtReplay::ReasoningBlocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::{FinishReason, ModelStreamEvent};
    use crate::provider::sse::SseEvents;
    use rho_harness_core::error::AppError;
    use rho_harness_core::model::Reasoning;
    use test_case::test_case;

    fn feed_sse(lines: &[&str]) -> SseEvents {
        let mut parser = sse_parser();
        let mut payload = String::new();
        for line in lines {
            payload.push_str("data: ");
            payload.push_str(line);
            payload.push_str("\n\n");
        }
        parser.feed(payload.as_bytes())
    }

    #[test]
    fn streams_thoughts_emits_reasoning_block_and_attaches_signature_to_tool_call() {
        let events = feed_sse(&[
            r#"{"candidates":[{"content":{"parts":[{"text":"Step 1. ","thought":true}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"Step 2.","thought":true,"thoughtSignature":"YWJjZA=="}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"id":"fc-1","name":"read","args":{"path":"lib.rs"}},"thoughtSignature":"ZWZnaA=="}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":120,"candidatesTokenCount":25,"thoughtsTokenCount":15,"cachedContentTokenCount":80,"totalTokenCount":160}}"#,
        ]);

        assert_eq!(events.len(), 6);
        assert_eq!(
            events[0].as_ref().unwrap(),
            &ModelStreamEvent::Reasoning("Step 1. ".to_string())
        );
        assert_eq!(
            events[1].as_ref().unwrap(),
            &ModelStreamEvent::Reasoning("Step 2.".to_string())
        );
        assert_eq!(
            events[2].as_ref().unwrap(),
            &ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
                "Step 1. Step 2.",
                Some("YWJjZA==".to_string())
            ))
        );
        match events[3].as_ref().unwrap() {
            ModelStreamEvent::ToolCall(call) => {
                assert_eq!(call.id, "fc-1");
                assert_eq!(call.function.name, "read");
                assert_eq!(call.signature.as_deref(), Some("ZWZnaA=="));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match events[4].as_ref().unwrap() {
            ModelStreamEvent::Usage(u) => {
                assert_eq!(u.input_tokens, 40); // 120 - 80 cached
                assert_eq!(u.output_tokens, 40); // 25 + 15 thoughts
                assert_eq!(u.cached_input_tokens, Some(80));
                assert_eq!(u.reasoning_tokens, Some(15));
            }
            other => panic!("expected Usage, got {other:?}"),
        }
        assert_eq!(
            events[5].as_ref().unwrap(),
            &ModelStreamEvent::Finish(FinishReason::Stop)
        );
    }

    #[test]
    fn malformed_function_call_surfaces_as_retryable_provider_unavailable() {
        let events = feed_sse(&[r#"{"candidates":[{"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#]);
        assert!(matches!(
            events.last(),
            Some(Err(AppError::ProviderUnavailable { message, .. })) if message.contains("MALFORMED_FUNCTION_CALL")
        ));
    }

    async fn collect_body(payloads: &[&str]) -> SseEvents {
        crate::provider::sse::collect_sse_payloads(payloads, sse_parser()).await
    }

    #[tokio::test]
    async fn stream_closed_before_finish_reason_is_a_network_error() {
        let events = collect_body(&[r#"{"candidates":[{"content":{"parts":[{"text":"cut off"}]}}]}"#]).await;
        assert!(matches!(&events[0], Ok(ModelStreamEvent::Text(t)) if t == "cut off"));
        assert!(matches!(events.last(), Some(Err(AppError::Network(_)))), "{events:?}");
    }

    #[tokio::test]
    async fn stream_closed_after_finish_reason_is_complete() {
        let events =
            collect_body(&[r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#]).await;
        assert!(events.iter().all(Result::is_ok), "{events:?}");
        assert!(matches!(
            events.last(),
            Some(Ok(ModelStreamEvent::Finish(FinishReason::Stop)))
        ));
    }

    #[tokio::test]
    async fn usage_in_a_chunk_after_the_finish_reason_is_delivered() {
        let events = collect_body(&[
            r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#,
            r#"{"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}"#,
        ])
        .await;
        assert!(events.iter().all(Result::is_ok), "{events:?}");
        assert!(matches!(events.last(), Some(Ok(ModelStreamEvent::Usage(u))) if u.total_tokens == 6));
    }

    fn usage_totals(events: &SseEvents) -> Vec<u64> {
        events
            .iter()
            .filter_map(|e| match e {
                Ok(ModelStreamEvent::Usage(u)) => Some(u.total_tokens),
                _ => None,
            })
            .collect()
    }

    #[test_case(
        &[
            r#"{"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":1,"totalTokenCount":5}}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}"#,
        ];
        "usage grows into the finishing chunk"
    )]
    #[test_case(
        &[
            r#"{"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#,
        ];
        "usage before a finishing chunk without usage"
    )]
    #[tokio::test]
    async fn cumulative_usage_is_reported_once_with_the_latest_totals(payloads: &[&str]) {
        let events = collect_body(payloads).await;
        assert_eq!(usage_totals(&events), [6]);
    }

    #[test_case(r#"{"error":{"code":429,"message":"Individual quota reached."}}"#, "provider"; "quota")]
    #[test_case(r#"{"error":{"code":429,"message":"slow down"}}"#, "rate_limited"; "rate limit")]
    #[test_case(
        r#"{"error":{"code":400,"message":"The input token count exceeds the maximum"}}"#,
        "context_overflow";
        "context overflow"
    )]
    #[test_case(r#"{"error":{"code":503,"message":"overloaded"}}"#, "unavailable"; "unavailable")]
    fn in_band_errors_classify_quota_overflow_and_rate_limits(line: &str, expected: &str) {
        let kind = match feed_sse(&[line]).as_slice() {
            [Err(AppError::Provider(_))] => "provider",
            [Err(AppError::RateLimited { .. })] => "rate_limited",
            [Err(AppError::ContextOverflow(_))] => "context_overflow",
            [Err(AppError::ProviderUnavailable { .. })] => "unavailable",
            other => panic!("{other:?}"),
        };
        assert_eq!(kind, expected);
    }

    #[test]
    fn maps_max_tokens_finish_reason() {
        let events =
            feed_sse(&[r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"MAX_TOKENS"}]}"#]);
        assert_eq!(
            events.last().unwrap().as_ref().unwrap(),
            &ModelStreamEvent::Finish(FinishReason::Length)
        );
    }
}
