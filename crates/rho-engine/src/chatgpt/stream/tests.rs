use super::*;
use crate::provider::adapter::ModelStreamEvent;
use rho_harness_core::error::AppError;

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
    let mut input = Vec::new();

    for event in events {
        match event.unwrap() {
            ModelStreamEvent::Text(t) if t == "Let me run that." => has_text = true,
            ModelStreamEvent::ToolInputDelta { id, name, json } => input.push((id, name, json)),
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
    let fragments: Vec<&str> = input.iter().map(|(_, _, json)| json.as_str()).collect();
    assert_eq!(fragments, ["", "{\"command\": \"", "cargo check\"}"]);
    assert!(input.iter().all(|(id, name, _)| id == "call_1" && name == "bash"));
}

#[test]
fn parallel_function_calls_with_interleaved_arguments_are_each_emitted() {
    let call = |kind: &str, index: u64, id: &str| {
        format!(
            "data: {{\"type\":\"response.output_item.{kind}\",\"output_index\":{index},\"item\":{{\"type\":\"function_call\",\"id\":\"fc_{id}\",\"call_id\":\"call_{id}\",\"name\":\"read\"}}}}\n\n"
        )
    };
    let delta = |id: &str, text: &str| {
        format!(
            "data: {{\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_{id}\",\"delta\":\"{text}\"}}\n\n"
        )
    };
    let chunks = [
        call("added", 0, "a"),
        call("added", 1, "b"),
        delta("b", "{\\\"path\\\":\\\"b.rs\\\"}"),
        delta("a", "{\\\"path\\\":\\\"a.rs\\\"}"),
        call("done", 0, "a"),
        call("done", 1, "b"),
        COMPLETED.to_string(),
    ];
    let mut parser = SseParser::new();
    let calls: Vec<_> = chunks
        .iter()
        .flat_map(|chunk| parser.feed(chunk.as_bytes()))
        .filter_map(|event| match event {
            Ok(ModelStreamEvent::ToolCall(call)) => Some((call.id, call.function.arguments)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        [
            ("call_a".to_string(), serde_json::json!({"path": "a.rs"})),
            ("call_b".to_string(), serde_json::json!({"path": "b.rs"})),
        ]
    );
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
        finish: FinishReason::Other,
        usage: None,
    },
    FinishCase {
        line: r#"{"type": "response.incomplete", "response": {"incomplete_details": {"reason": "max_output_tokens"}, "usage": {"input_tokens": 3, "output_tokens": 7, "total_tokens": 10}}}"#,
        finish: FinishReason::Length,
        usage: Some((3, 10)),
    },
    FinishCase {
        line: r#"{"type": "response.incomplete", "response": {"incomplete_details": {"reason": "content_filter"}}}"#,
        finish: FinishReason::ContentFilter,
        usage: None,
    },
    FinishCase {
        line: r#"{"type": "response.incomplete", "response": {"incomplete_details": {"reason": "something_new"}}}"#,
        finish: FinishReason::Other,
        usage: None,
    },
    FinishCase {
        line: r#"{"type": "response.incomplete", "response": {"incomplete_details": null}}"#,
        finish: FinishReason::Other,
        usage: None,
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
fn finished_reasoning_item_with_encrypted_content_becomes_a_replayable_block() {
    use rho_harness_core::model::{Reasoning, ReasoningContent};
    let events = feed_events(&[
        r#"{"type": "response.output_item.done", "output_index": 0, "item": {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "Plan"}, {"type": "summary_text", "text": "Act"}], "encrypted_content": "ENC"}}"#,
    ]);
    let expected = Reasoning {
        id: Some("rs_1".to_string()),
        content: vec![
            ReasoningContent::Summary("Plan".to_string()),
            ReasoningContent::Summary("Act".to_string()),
            ReasoningContent::Encrypted("ENC".to_string()),
        ],
    };
    assert_eq!(
        events.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![ModelStreamEvent::ReasoningBlock(expected)]
    );
}

#[test]
fn finished_reasoning_item_without_encrypted_content_is_not_replayable() {
    let events = feed_events(&[
        r#"{"type": "response.output_item.done", "output_index": 0, "item": {"type": "reasoning", "id": "rs_1", "summary": []}}"#,
    ]);
    assert!(events.is_empty());
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
            if u.input_tokens == 3
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

async fn collect_body(chunks: &[&str]) -> Vec<Result<ModelStreamEvent, AppError>> {
    use futures::StreamExt;
    let body = futures::stream::iter(
        chunks
            .iter()
            .map(|c| Ok::<_, reqwest::Error>(c.as_bytes().to_vec()))
            .collect::<Vec<_>>(),
    );
    crate::provider::sse::unfold_sse_stream(body, SseParser::new())
        .collect()
        .await
}

#[tokio::test]
async fn stream_closed_without_terminal_event_is_a_network_error() {
    let events = collect_body(&["data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n"]).await;
    assert!(has_event(&events, |e| matches!(e, ModelStreamEvent::Text(_))));
    assert!(
        matches!(events.last(), Some(Err(AppError::Network(m))) if m == "ChatGPT stream closed before a terminal event")
    );
}

#[tokio::test]
async fn stream_closed_with_unfinished_tool_call_reports_it() {
    let events = collect_body(&[concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,",
        "\"item\":{\"type\":\"function_call\",\"id\":\"fc1\",\"call_id\":\"c1\",\"name\":\"bash\"}}\n\n"
    )])
    .await;
    assert!(matches!(events.last(), Some(Err(AppError::Network(m))) if m.contains("1 unfinished tool call")));
}

#[tokio::test]
async fn stream_with_completed_event_ends_without_error() {
    let events = collect_body(&["data: {\"type\":\"response.completed\",\"response\":{}}\n\n"]).await;
    assert!(events.iter().all(Result::is_ok));
    assert!(has_event(&events, |e| matches!(e, ModelStreamEvent::Finish(_))));
}

fn item(kind: &str, fields: &str) -> String {
    format!(
        "data: {{\"type\":\"response.output_item.{kind}\",\"output_index\":3,\"item\":{{\"type\":\"function_call\"{fields}}}}}\n\n"
    )
}

async fn tool_call_from(chunks: &[String]) -> (String, String, serde_json::Value) {
    let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let calls: Vec<_> = collect_body(&refs)
        .await
        .into_iter()
        .filter_map(|e| match e {
            Ok(ModelStreamEvent::ToolCall(call)) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    (
        call.id.clone(),
        call.function.name.clone(),
        call.function.arguments.clone(),
    )
}

const COMPLETED: &str = "data: {\"type\":\"response.completed\",\"response\":{}}\n\n";

#[tokio::test]
async fn done_event_without_ids_or_arguments_falls_back_to_the_pending_call() {
    let added = item("added", ",\"id\":\"fc1\",\"call_id\":\"c1\",\"name\":\"bash\"");
    let done = item("done", ",\"id\":\"fc1\"");
    let delta = "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc1\",\"delta\":\"{\\\"a\\\":1}\"}\n\n";
    let (id, name, args) = tool_call_from(&[added, delta.into(), done, COMPLETED.into()]).await;
    assert_eq!(
        (id.as_str(), name.as_str(), args),
        ("c1", "bash", serde_json::json!({"a": 1}))
    );
}

#[tokio::test]
async fn done_event_values_win_over_the_pending_call() {
    let added = item(
        "added",
        ",\"id\":\"fc1\",\"call_id\":\"old\",\"name\":\"bash\",\"arguments\":\"{\\\"a\\\":1}\"",
    );
    let done = item(
        "done",
        ",\"id\":\"fc1\",\"call_id\":\"new\",\"name\":\"bash\",\"arguments\":\"{\\\"a\\\":2}\"",
    );
    let (id, _, args) = tool_call_from(&[added, done, COMPLETED.into()]).await;
    assert_eq!((id.as_str(), args), ("new", serde_json::json!({"a": 2})));
}

#[tokio::test]
async fn pending_wire_id_is_used_when_the_done_event_has_none() {
    let added = item("added", ",\"id\":\"output-3\",\"name\":\"bash\"");
    let done = item("done", ",\"name\":\"bash\"");
    let (id, _, _) = tool_call_from(&[added, done, COMPLETED.into()]).await;
    assert_eq!(id, "output-3");
}

#[tokio::test]
async fn call_id_falls_back_to_the_wire_id_then_to_the_output_index() {
    let with_wire_id = item("done", ",\"id\":\"fc9\",\"name\":\"bash\"");
    let (id, _, _) = tool_call_from(&[with_wire_id, COMPLETED.into()]).await;
    assert_eq!(id, "fc9");

    let anonymous = item("done", ",\"name\":\"bash\"");
    let (id, _, _) = tool_call_from(&[anonymous, COMPLETED.into()]).await;
    assert_eq!(id, "call-3");
}

#[tokio::test]
async fn truncated_tool_arguments_are_kept_raw_instead_of_replaced_with_an_empty_object() {
    let done = item(
        "done",
        ",\"id\":\"fc1\",\"call_id\":\"c1\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\": \\\"src/lib\"",
    );
    let (_, _, args) = tool_call_from(&[done, COMPLETED.into()]).await;
    assert_eq!(args, serde_json::json!({"raw": "{\"path\": \"src/lib"}));
}

#[tokio::test]
async fn missing_tool_arguments_become_an_empty_object() {
    let done = item("done", ",\"id\":\"fc1\",\"call_id\":\"c1\",\"name\":\"read\"");
    let (_, _, args) = tool_call_from(&[done, COMPLETED.into()]).await;
    assert_eq!(args, serde_json::json!({}));
}
