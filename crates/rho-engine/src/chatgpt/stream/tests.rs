use super::*;
use crate::provider::adapter::ModelStreamEvent;

#[test]
fn reasoning_summary_parts_inject_paragraph_breaks_between_steps() {
    let mut parser = SseParser::new();

    let chunks = [
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"summary_index\": 0, \"delta\": \"Thinking about the plan.\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"summary_index\": 1, \"delta\": \"Step 1: Check code.\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"summary_index\": 2}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"summary_index\": 2, \"delta\": \"Step 2: Run tests.\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"summary_index\": 2}\n\n",
        "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 10, \"output_tokens\": 20, \"total_tokens\": 30}}}\n\n",
    ];

    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk.as_bytes()));
    }

    let mut reasoning_pieces = Vec::new();
    for event in &events {
        if let Ok(ModelStreamEvent::Reasoning(text)) = event {
            reasoning_pieces.push(text.as_str());
        }
    }

    assert_eq!(
        reasoning_pieces,
        vec![
            "Thinking about the plan.",
            "\n\n",
            "Step 1: Check code.",
            "\n\n",
            "Step 2: Run tests."
        ]
    );

    let full_reasoning = reasoning_pieces.concat();
    assert_eq!(
        full_reasoning,
        "Thinking about the plan.\n\nStep 1: Check code.\n\nStep 2: Run tests."
    );
}

#[test]
fn text_and_tool_calls_stream_and_aggregate() {
    let mut parser = SseParser::new();

    let chunks = [
        "data: {\"type\": \"response.output_text.delta\", \"delta\": \"Let me run that.\"}\n\n",
        "data: {\"type\": \"response.output_item.added\", \"output_index\": 1, \"item\": {\"type\": \"function_call\", \"id\": \"fc_1\", \"call_id\": \"call_1\", \"name\": \"bash\", \"arguments\": \"\"}}\n\n",
        "data: {\"type\": \"response.function_call_arguments.delta\", \"output_index\": 1, \"item_id\": \"fc_1\", \"delta\": \"{\\\"command\\\": \\\"\"}\n\n",
        "data: {\"type\": \"response.function_call_arguments.delta\", \"output_index\": 1, \"item_id\": \"fc_1\", \"delta\": \"cargo check\\\"}\"}\n\n",
        "data: {\"type\": \"response.output_item.done\", \"output_index\": 1, \"item\": {\"type\": \"function_call\", \"id\": \"fc_1\", \"call_id\": \"call_1\", \"name\": \"bash\", \"arguments\": \"\"}}\n\n",
        "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 50, \"output_tokens\": 15, \"total_tokens\": 65, \"output_tokens_details\": {\"reasoning_tokens\": 0}}}}\n\n",
    ];

    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk.as_bytes()));
    }

    let mut has_text = false;
    let mut tool_call = None;
    let mut total_tokens = None;

    for event in events {
        match event.unwrap() {
            ModelStreamEvent::Text(t) if t == "Let me run that." => has_text = true,
            ModelStreamEvent::ToolCall(call) => {
                tool_call = Some((call.id, call.function.name, call.function.arguments));
            }
            ModelStreamEvent::Usage(usage) => total_tokens = Some(usage.total_tokens),
            _ => {}
        }
    }

    assert!(has_text);
    assert_eq!(total_tokens, Some(65));
    let (id, name, args) = tool_call.unwrap();
    assert_eq!(id, "call_1");
    assert_eq!(name, "bash");
    assert_eq!(args, serde_json::json!({"command": "cargo check"}));
}

#[test]
fn reasoning_summary_across_multiple_output_items_does_not_collide() {
    let mut parser = SseParser::new();

    let chunks = [
        "data: {\"type\": \"response.output_item.added\", \"output_index\": 0, \"item\": {\"type\": \"reasoning\", \"id\": \"rs_0\"}}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"output_index\": 0, \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 0, \"summary_index\": 0, \"delta\": \"**Designing durable task reconciliation**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"output_index\": 0, \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"output_index\": 0, \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 0, \"summary_index\": 1, \"delta\": \"**Planning startup task refresher invocation**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"output_index\": 0, \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"output_index\": 0, \"summary_index\": 2}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 0, \"summary_index\": 2, \"delta\": \"**Evaluating job schedule frequency trade-offs**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"output_index\": 0, \"summary_index\": 2}\n\n",
        "data: {\"type\": \"response.output_item.done\", \"output_index\": 0, \"item\": {\"type\": \"reasoning\", \"id\": \"rs_0\"}}\n\n",
        "data: {\"type\": \"response.output_item.added\", \"output_index\": 1, \"item\": {\"type\": \"reasoning\", \"id\": \"rs_1\"}}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"output_index\": 1, \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 1, \"summary_index\": 0, \"delta\": \"**Implementing task reconciliation at startup**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"output_index\": 1, \"summary_index\": 0}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.added\", \"output_index\": 1, \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 1, \"summary_index\": 1, \"delta\": \"**Adding automated tests**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_part.done\", \"output_index\": 1, \"summary_index\": 1}\n\n",
        "data: {\"type\": \"response.output_item.done\", \"output_index\": 1, \"item\": {\"type\": \"reasoning\", \"id\": \"rs_1\"}}\n\n",
        "data: {\"type\": \"response.completed\", \"response\": {\"usage\": {\"input_tokens\": 10, \"output_tokens\": 20, \"total_tokens\": 30}}}\n\n",
    ];

    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk.as_bytes()));
    }

    let mut reasoning_pieces = Vec::new();
    for event in &events {
        if let Ok(ModelStreamEvent::Reasoning(text)) = event {
            reasoning_pieces.push(text.as_str());
        }
    }

    let full_reasoning = reasoning_pieces.concat();
    assert!(full_reasoning.contains("**Designing durable task reconciliation**\n\n**Planning startup task refresher invocation**\n\n**Evaluating job schedule frequency trade-offs**\n\n**Implementing task reconciliation at startup**\n\n**Adding automated tests**"));
}

fn feed_events(lines: &[&str]) -> Vec<Result<ModelStreamEvent, AppError>> {
    let mut parser = SseParser::new();
    let mut events = Vec::new();
    for line in lines {
        events.extend(parser.feed(format!("data: {line}\n\n").as_bytes()));
    }
    events
}

fn has_event(events: &[Result<ModelStreamEvent, AppError>], pred: impl Fn(&ModelStreamEvent) -> bool) -> bool {
    events.iter().any(|e| e.as_ref().is_ok_and(&pred))
}

const ERROR_CASES: &[(&str, &str)] = &[
    (
        r#"{"type": "response.failed", "response": {"error": {"message": "model overloaded"}}}"#,
        "model overloaded",
    ),
    (
        r#"{"type": "response.failed", "response": {"error": {"message": ""}}}"#,
        "ChatGPT response failed",
    ),
    (r#"{"type": "response.failed"}"#, "ChatGPT response failed"),
    (
        r#"{"type": "error", "error": {"message": "rate limited"}, "message": "ignored"}"#,
        "rate limited",
    ),
    (
        r#"{"type": "error", "message": "top level failure"}"#,
        "top level failure",
    ),
    (r#"{"type": "error"}"#, "ChatGPT API error"),
];

struct FinishCase {
    line: &'static str,
    finish: FinishReason,
    usage: Option<(u64, u64)>,
}

const FINISH_CASES: &[FinishCase] = &[
    FinishCase {
        line: r#"{"type": "response.completed"}"#,
        finish: FinishReason::Stop,
        usage: Some((0, 0)),
    },
    FinishCase {
        line: r#"{"type": "response.incomplete"}"#,
        finish: FinishReason::Length,
        usage: None,
    },
    FinishCase {
        line: r#"{"type": "response.incomplete", "response": {"usage": {"input_tokens": 3, "output_tokens": 7, "total_tokens": 10}}}"#,
        finish: FinishReason::Length,
        usage: Some((3, 10)),
    },
];

#[test]
fn reasoning_summary_delta_and_done_variants_are_interpreted() {
    let events = feed_events(&[
        r#"{"type": "response.reasoning_summary.delta", "summary_index": 0, "delta": "alpha"}"#,
        r#"{"type": "response.reasoning_summary_part.done"}"#,
        r#"{"type": "response.reasoning_summary_text.done"}"#,
        r#"{"type": "response.reasoning_summary.done"}"#,
        r#"{"type": "response.reasoning_text.delta", "delta": "beta"}"#,
    ]);
    let reasoning: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Ok(ModelStreamEvent::Reasoning(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasoning, vec!["alpha", "beta"]);
}

#[test]
fn reasoning_summary_part_added_without_output_index_emits_no_error() {
    let events = feed_events(&[
        r#"{"type": "response.reasoning_summary_part.added", "summary_index": 0}"#,
        r#"{"type": "response.reasoning_summary_text.delta", "summary_index": 0, "delta": "gamma"}"#,
    ]);
    assert!(events.iter().all(|e| e.is_ok()));
    assert!(has_event(
        &events,
        |e| matches!(e, ModelStreamEvent::Reasoning(t) if t == "gamma")
    ));
}

#[test]
fn non_function_output_items_are_ignored_for_tool_calls() {
    let events = feed_events(&[
        r#"{"type": "response.output_item.added", "output_index": 0, "item": {"type": "message", "id": "msg_1"}}"#,
        r#"{"type": "response.output_item.done", "output_index": 0, "item": {"type": "message", "id": "msg_1"}}"#,
        r#"{"type": "response.output_text.delta", "delta": "hi"}"#,
    ]);
    assert!(!has_event(&events, |e| matches!(e, ModelStreamEvent::ToolCall(_))));
    assert!(has_event(
        &events,
        |e| matches!(e, ModelStreamEvent::Text(t) if t == "hi")
    ));
}

#[test]
fn response_completed_usage_maps_cached_and_reasoning_tokens() {
    let events = feed_events(&[
        r#"{"type": "response.completed", "response": {"usage": {"input_tokens": 5, "output_tokens": 6, "total_tokens": 11, "input_tokens_details": {"cached_tokens": 2}, "output_tokens_details": {"reasoning_tokens": 3}}}}"#,
    ]);
    assert!(has_event(&events, |e| matches!(
        e,
        ModelStreamEvent::Usage(u)
            if u.input_tokens == 5
                && u.output_tokens == 6
                && u.total_tokens == 11
                && u.cached_input_tokens == Some(2)
                && u.reasoning_tokens == Some(3)
    )));
}

#[test]
fn terminal_events_finish_with_reason_and_usage() {
    for case in FINISH_CASES {
        let events = feed_events(&[case.line]);
        let finish = case.finish;
        assert!(
            has_event(&events, |e| matches!(e, ModelStreamEvent::Finish(f) if *f == finish)),
            "{}",
            case.line
        );
        if let Some((input, total)) = case.usage {
            assert!(
                has_event(
                    &events,
                    |e| matches!(e, ModelStreamEvent::Usage(u) if u.input_tokens == input && u.total_tokens == total)
                ),
                "{}",
                case.line
            );
        }
    }
}

#[test]
fn error_events_surface_provider_errors() {
    for &(line, expected) in ERROR_CASES {
        let events = feed_events(&[line]);
        assert!(
            matches!(events.as_slice(), [Err(AppError::Provider(msg))] if msg == expected),
            "{line}"
        );
    }
}

#[test]
fn unknown_and_malformed_events_are_ignored() {
    let events = feed_events(&[r#"{"type": "response.something_new"}"#, r#"{"not_type": true}"#]);
    assert!(events.is_empty());
}
