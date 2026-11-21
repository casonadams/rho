use super::types::{CompletionCall, FinishReason, PromptResponse, Usage};
use super::*;

fn usage() -> Usage {
    let mut u = Usage::new();
    u.input_tokens = Some(10);
    u.output_tokens = Some(4);
    u.total_tokens = Some(14);
    u.cached_input_tokens = Some(3);
    u.cache_creation_input_tokens = Some(2);
    u.reasoning_tokens = Some(5);
    u
}

#[test]
fn usage_records_optional_cache_and_reasoning_only_when_reported() {
    let available = StructuralUsage::from(usage());
    assert_eq!(available.cached_input_tokens, Some(3));
    assert_eq!(available.reasoning_tokens, Some(5));

    let mut u = Usage::new();
    u.input_tokens = Some(2);
    u.output_tokens = Some(1);
    u.total_tokens = Some(3);
    let absent = StructuralUsage::from(u);
    let encoded = serde_json::to_string(&absent).unwrap();
    assert!(!encoded.contains("cached_input_tokens"));
    assert!(!encoded.contains("reasoning_tokens"));
}

#[test]
fn tracker_counts_tool_errors_and_denials_separately() {
    let tracker = RunTracker::default();
    tracker.start("model");
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
    tracker.start("model");
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
        CompletionCall::new(0, usage()).with_finish_reason(Some(FinishReason::Other)),
    ]);
    let tracker = RunTracker::default();
    tracker.start("model");
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
fn run_summary_records_the_model_and_when_each_request_was_sent() {
    let tracker = RunTracker::default();
    assert_eq!(tracker.last_request_started(), None);
    tracker.start("claude-opus-5-5");
    let sent = std::time::Instant::now();
    tracker.completion(CompletionCall::new(0, usage()), sent);

    let metrics = tracker.terminate("session", TerminalStatus::Cancelled);

    assert_eq!(metrics.model, "claude-opus-5-5");
    assert_eq!(metrics.completion_calls.len(), 1);
    assert!(metrics.completion_calls[0].started_ms.is_some());
    assert_eq!(tracker.last_request_started(), Some(sent));
}

#[test]
fn cached_prompt_usage_reports_only_the_uncached_input() {
    let usage = StructuralUsage::from_cached_prompt(Some(100), Some(7), Some(107), Some(60), Some(3));
    assert_eq!(usage.input_tokens, 40);
    assert_eq!(usage.output_tokens, 7);
    assert_eq!(usage.total_tokens, 107);
    assert_eq!(usage.cached_input_tokens, Some(60));
    assert_eq!(usage.reasoning_tokens, Some(3));

    let missing = StructuralUsage::from_cached_prompt(Some(5), None, None, Some(9), None);
    assert_eq!(
        (missing.input_tokens, missing.output_tokens, missing.total_tokens),
        (0, 0, 0)
    );
    assert_eq!(missing.cache_creation_input_tokens, None);
}

#[test]
fn merged_usage_keeps_optional_counts_reported_by_either_side() {
    let left = StructuralUsage {
        cached_input_tokens: Some(3),
        ..StructuralUsage::default()
    };
    let right = StructuralUsage {
        cached_input_tokens: Some(4),
        reasoning_tokens: Some(5),
        ..StructuralUsage::default()
    };
    let merged = left.merge(&right);
    assert_eq!(merged.cached_input_tokens, Some(7));
    assert_eq!(merged.reasoning_tokens, Some(5));
    assert_eq!(merged.cache_creation_input_tokens, None);
}

#[test]
fn format_tokens_breakpoints() {
    use super::types::format_tokens;

    let cases = [
        (0, "0"),
        (999, "999"),
        (1_000, "1.0k"),
        (1_234, "1.2k"),
        (32_768, "32k"),
        (45_300, "45.3k"),
        (99_949, "99.9k"),
        (128_000, "128k"),
        (131_072, "128k"),
        (262_144, "256k"),
        (1_000_000, "1M"),
        (1_048_576, "1M"),
        (2_500_000, "2.5M"),
    ];
    for (tokens, expected) in cases {
        assert_eq!(format_tokens(tokens), expected, "{tokens} tokens");
    }
}
