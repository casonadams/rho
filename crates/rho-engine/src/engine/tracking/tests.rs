use super::*;
use crate::engine::metrics::StructuralUsage;

#[test]
fn usage_tracker_accumulates_totals_across_turns() {
    let tracker = UsageTracker::default();
    let turn1 = StructuralUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: Some(20),
        cache_creation_input_tokens: Some(10),
        tool_use_prompt_tokens: None,
        reasoning_tokens: Some(5),
    };
    let turn2 = StructuralUsage {
        input_tokens: 200,
        output_tokens: 80,
        total_tokens: 280,
        cached_input_tokens: Some(40),
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };

    tracker.record(turn1);
    tracker.record(turn2);

    let totals = tracker.totals();
    assert_eq!(totals.total_input, 300);
    assert_eq!(totals.total_output, 130);
    assert_eq!(totals.total_cache_read, 60);
    assert_eq!(totals.total_cache_write, 10);
    assert_eq!(totals.total_reasoning, 5);
    assert_eq!(tracker.latest(), Some(turn2));
}

#[test]
fn usage_tracker_record_turn_differentiates_totals_from_latest_context() {
    let tracker = UsageTracker::default();
    let multi_turn_total = StructuralUsage {
        input_tokens: 30_000,
        output_tokens: 1_200,
        total_tokens: 31_200,
        cached_input_tokens: Some(5_000),
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };
    let final_call_context = StructuralUsage {
        input_tokens: 11_000,
        output_tokens: 400,
        total_tokens: 11_400,
        cached_input_tokens: Some(5_000),
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };

    tracker.record_turn(TurnUsage::new(multi_turn_total, final_call_context), 2000);

    let totals = tracker.totals();
    assert_eq!(totals.total_input, 30_000);
    assert_eq!(totals.total_output, 1_200);

    // Latest must reflect the active context call, NOT the accumulated 30k input tokens
    let latest = tracker.latest().expect("latest usage exists");
    assert_eq!(latest.input_tokens, 11_000);
    assert_eq!(latest.output_tokens, 400);

    // Speed uses total output tokens / elapsed
    assert_eq!(tracker.tokens_per_second(), Some(600.0));
}

#[test]
fn speed_tracker_computes_rate_and_resets() {
    let mut speed = SpeedTracker::default();
    speed.record_generation(100, 2000);

    let tps = speed.tokens_per_second();
    assert_eq!(tps, Some(50.0));

    speed.reset();
    assert_eq!(speed.tokens_per_second(), None);
}

#[test]
fn quota_tracker_caching_and_backoff() {
    let tracker = QuotaTracker::default();
    assert!(tracker.should_fetch());

    tracker.record_success("85% (3h22m)".to_string());
    assert_eq!(tracker.latest(), Some("85% (3h22m)".to_string()));
    assert!(!tracker.should_fetch());

    let tracker_fail = QuotaTracker::default();
    tracker_fail.record_failure();
    // After failure, error_until is set in the future so should_fetch is false
    assert!(!tracker_fail.should_fetch());
}
