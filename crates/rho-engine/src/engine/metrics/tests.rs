use super::*;
use rig::agent::{CompletionCall, PromptResponse};
use rig::completion::{FinishReason, Usage};

fn usage() -> Usage {
    Usage {
        input_tokens: 10,
        output_tokens: 4,
        total_tokens: 14,
        cached_input_tokens: 3,
        cache_creation_input_tokens: 2,
        tool_use_prompt_tokens: 1,
        reasoning_tokens: 5,
    }
}

#[test]
fn usage_records_optional_cache_and_reasoning_only_when_reported() {
    let available = StructuralUsage::from(usage());
    assert_eq!(available.cached_input_tokens, Some(3));
    assert_eq!(available.reasoning_tokens, Some(5));

    let absent = StructuralUsage::from(Usage {
        input_tokens: 2,
        output_tokens: 1,
        total_tokens: 3,
        ..Usage::new()
    });
    let encoded = serde_json::to_string(&absent).unwrap();
    assert!(!encoded.contains("cached_input_tokens"));
    assert!(!encoded.contains("reasoning_tokens"));
}

#[test]
fn tracker_counts_tool_errors_and_denials_separately() {
    let tracker = RunTracker::default();
    tracker.start();
    tracker.tool_called();
    tracker.tool_finished("denied");
    tracker.tool_called();
    tracker.tool_finished("error");
    let metrics = tracker.terminate("session", TerminalStatus::Failed);

    assert_eq!(metrics.tool_calls, 2);
    assert_eq!(metrics.tool_errors, 2);
    assert_eq!(metrics.tool_denials, 1);
}

fn track_sample_run(session_id: &str, response: &PromptResponse) -> RunMetrics {
    let tracker = RunTracker::default();
    tracker.start();
    tracker.tool_called();
    tracker.tool_finished("success");
    tracker
        .complete(CompletionOutcome {
            session_id,
            status: TerminalStatus::Completed,
            response,
        })
        .normalized()
}

#[test]
fn normalized_metrics_are_stable_across_runs() {
    let response = PromptResponse::new("not recorded", usage()).with_completion_calls(vec![
        CompletionCall::new(0, usage()).with_finish_reason(Some(FinishReason::Stop)),
    ]);
    let first = track_sample_run("random-a", &response);
    let second = track_sample_run("random-b", &response);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn structural_metrics_contain_no_response_or_identity_content() {
    let sentinel = "credential-sentinel";
    let response = PromptResponse::new(sentinel, usage()).with_completion_calls(vec![
        CompletionCall::new(0, usage())
            .with_identity(rig::completion::ResponseIdentity {
                message_id: Some(sentinel.to_string()),
                response_id: Some(sentinel.to_string()),
                provider_request_id: Some(sentinel.to_string()),
            })
            .with_finish_reason(Some(FinishReason::Other(sentinel.to_string()))),
    ]);
    let tracker = RunTracker::default();
    tracker.start();
    let encoded = serde_json::to_string(&tracker.complete(CompletionOutcome {
        session_id: "safe-session",
        status: TerminalStatus::Completed,
        response: &response,
    }))
    .unwrap();

    assert!(!encoded.contains(sentinel));
    assert!(!encoded.contains("\"output\":"));
    assert!(!encoded.contains("message_id"));
}

#[test]
fn format_tokens_powers_of_two_and_decimal() {
    use super::types::format_tokens;

    assert_eq!(format_tokens(262_144), "256k");
    assert_eq!(format_tokens(131_072), "128k");
    assert_eq!(format_tokens(65_536), "64k");
    assert_eq!(format_tokens(32_768), "32k");
    assert_eq!(format_tokens(1_048_576), "1M");
    assert_eq!(format_tokens(128_000), "128k");
    assert_eq!(format_tokens(200_000), "200k");
    assert_eq!(format_tokens(1_000_000), "1M");
}
