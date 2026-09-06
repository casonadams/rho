use super::*;
use crate::engine::metrics::StructuralUsage;

fn make_usage(tokens: (u64, u64), cache: (Option<u64>, Option<u64>)) -> StructuralUsage {
    let (inp, out) = tokens;
    let (cr, cw) = cache;
    StructuralUsage {
        input_tokens: inp,
        output_tokens: out,
        total_tokens: inp + out,
        cached_input_tokens: cr,
        cache_creation_input_tokens: cw,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    }
}

#[test]
fn usage_tracker_accumulates_totals_across_turns() {
    let tracker = UsageTracker::default();
    let mut turn1 = make_usage((100, 50), (Some(20), Some(10)));
    turn1.reasoning_tokens = Some(5);
    let turn2 = make_usage((200, 80), (Some(40), None));

    tracker.record(turn1);
    tracker.record(turn2);

    let t = tracker.totals();
    let actual = (
        t.total_input,
        t.total_output,
        t.total_cache_read,
        t.total_cache_write,
        t.total_reasoning,
    );
    assert_eq!(actual, (300, 130, 60, 10, 5));
    assert_eq!(tracker.latest(), Some(turn2));
}

#[test]
fn usage_tracker_record_turn_differentiates_totals_from_latest_context() {
    let tracker = UsageTracker::default();
    let total = make_usage((30_000, 1_200), (Some(5_000), None));
    let final_ctx = make_usage((11_000, 400), (Some(5_000), None));
    tracker.record_turn(TurnUsage::new(total, final_ctx), 2000);

    let t = tracker.totals();
    assert_eq!((t.total_input, t.total_output), (30_000, 1_200));
    let latest = tracker.latest().unwrap();
    assert_eq!((latest.input_tokens, latest.output_tokens), (11_000, 400));
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
    let key = QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
    assert!(tracker.should_fetch(&key));

    tracker.record_success(&key, "85% (3h22m)".to_string());
    assert_eq!(tracker.display_for(&key), Some("85% (3h22m)".to_string()));
    assert!(!tracker.should_fetch(&key));

    let tracker_fail = QuotaTracker::default();
    tracker_fail.record_failure(&key);
    assert!(!tracker_fail.should_fetch(&key));
}

#[test]
fn quota_tracker_multi_provider_isolation() {
    let tracker = QuotaTracker::default();
    let ag_key = QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
    let local_key = QuotaKey::new("local", None::<String>);
    let ollama_key = QuotaKey::new("ollama-cloud", None::<String>);

    tracker.record_success(&ag_key, "85% (3h22m)".to_string());
    assert_eq!(tracker.display_for(&ag_key), Some("85% (3h22m)".to_string()));
    assert_eq!(tracker.display_for(&local_key), None);
    assert_eq!(tracker.display_for(&ollama_key), None);
    assert!(tracker.should_fetch(&ollama_key));
}

#[test]
fn quota_tracker_fallback_and_failure_isolation() {
    let tracker = QuotaTracker::default();
    let ollama_key = QuotaKey::new("ollama-cloud", None::<String>);
    let ollama_model_key = QuotaKey::new("ollama-cloud", Some("glm-5.3-flash"));
    tracker.record_success(&ollama_key, "20% used".to_string());
    assert_eq!(tracker.display_for(&ollama_key), Some("20% used".to_string()));
    assert_eq!(tracker.display_for(&ollama_model_key), Some("20% used".to_string()));

    let ag_fail_key = QuotaKey::new("antigravity", Some("claude-sonnet-4-6"));
    tracker.record_failure(&ag_fail_key);
    assert!(!tracker.should_fetch(&ag_fail_key));
    assert!(!tracker.should_fetch(&ollama_key));
}

#[test]
fn usage_tracker_in_flight_streaming() {
    let tracker = UsageTracker::default();
    tracker.start_turn(Some(500));
    assert_eq!((tracker.totals().total_input, tracker.totals().total_output), (500, 0));

    tracker.record_streaming_chunk(15);
    tracker.record_streaming_chunk(10);
    assert_eq!((tracker.totals().total_input, tracker.totals().total_output), (500, 25));
}

#[test]
fn usage_tracker_step_and_turn_reconciliation() {
    let tracker = UsageTracker::default();
    let step = make_usage((520, 28), (Some(100), Some(50)));
    tracker.record_step(step, 500);

    let t = tracker.totals();
    assert_eq!(
        (t.total_input, t.total_output, t.total_cache_read, t.total_cache_write),
        (520, 28, 100, 50)
    );
    let latest = tracker.latest().unwrap();
    assert_eq!((latest.input_tokens, latest.cached_input_tokens), (520, Some(100)));

    tracker.record_turn(TurnUsage::single(step), 500);
    let t = tracker.totals();
    assert_eq!(
        (t.total_input, t.total_output, t.total_cache_read, t.total_cache_write),
        (520, 28, 100, 50)
    );
}

#[test]
fn usage_tracker_in_flight_multi_step_progression() {
    let tracker = UsageTracker::default();
    tracker.start_turn(Some(1000));
    tracker.record_streaming_chunk(10);
    tracker.record_step(make_usage((1000, 15), (None, None)), 200);
    assert_eq!(
        (tracker.totals().total_input, tracker.totals().total_output),
        (1000, 15)
    );

    tracker.start_step();
    tracker.record_streaming_chunk(20);
    assert_eq!(tracker.totals().total_output, 35);

    tracker.record_step(make_usage((1200, 30), (None, None)), 300);
    assert_eq!(
        (tracker.totals().total_input, tracker.totals().total_output),
        (2200, 45)
    );
    assert_eq!(tracker.latest().unwrap().input_tokens, 1200);
}

#[test]
fn usage_tracker_guard_commits_partial_on_drop() {
    let tracker = UsageTracker::default();

    {
        let _guard = tracker.in_flight_guard();
        tracker.start_turn(Some(200));

        let step1 = StructuralUsage {
            input_tokens: 200,
            output_tokens: 50,
            total_tokens: 250,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };
        tracker.record_step(step1, 200);
    }

    let totals = tracker.totals();
    assert_eq!(totals.total_input, 200);
    assert_eq!(totals.total_output, 50);
}

#[test]
fn usage_tracker_guard_clears_uncompleted_on_drop() {
    let tracker = UsageTracker::default();

    {
        let _guard = tracker.in_flight_guard();
        tracker.start_turn(Some(200));
        tracker.record_streaming_chunk(5);
    }

    let totals = tracker.totals();
    assert_eq!(totals.total_input, 0);
    assert_eq!(totals.total_output, 0);
    assert_eq!(tracker.latest(), None);
}

#[test]
fn usage_tracker_tokens_per_second_during_streaming() {
    let tracker = UsageTracker::default();
    tracker.start_turn(Some(100));
    tracker.record_streaming_chunk(50);
    assert_eq!(tracker.tokens_per_second(), None);

    let step = StructuralUsage {
        input_tokens: 100,
        output_tokens: 100,
        total_tokens: 200,
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };
    tracker.record_step(step, 500);
    assert_eq!(tracker.tokens_per_second(), Some(200.0));
}

#[test]
fn usage_tracker_step_start_only_on_first_streaming_chunk() {
    let tracker = UsageTracker::default();
    tracker.start_turn(Some(100));
    // Turn started but no tokens streamed yet - step_start should be None and tps None
    assert_eq!(tracker.tokens_per_second(), None);

    // When text or reasoning chunks stream in, step_start begins
    tracker.record_streaming_chunk(25);

    // After step is recorded with duration, speed is stable
    let step = StructuralUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };
    tracker.record_step(step, 250);
    assert_eq!(tracker.tokens_per_second(), Some(200.0));
}
