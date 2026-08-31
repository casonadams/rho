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

#[test]
fn normalized_metrics_are_stable_across_runs() {
    let response = PromptResponse::new("not recorded", usage()).with_completion_calls(vec![
        CompletionCall::new(0, usage()).with_finish_reason(Some(FinishReason::Stop)),
    ]);
    let first = RunTracker::default();
    first.start();
    first.tool_called();
    first.tool_finished("success");
    let first = first
        .complete(CompletionOutcome {
            session_id: "random-a",
            status: TerminalStatus::Completed,
            response: &response,
        })
        .normalized();
    let second = RunTracker::default();
    second.start();
    second.tool_called();
    second.tool_finished("success");
    let second = second
        .complete(CompletionOutcome {
            session_id: "random-b",
            status: TerminalStatus::Completed,
            response: &response,
        })
        .normalized();

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
