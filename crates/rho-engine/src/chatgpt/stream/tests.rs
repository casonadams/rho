use super::*;
use rig::message::AssistantContent;
use rig::streaming::RawStreamingChoice;

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
        if let Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) = event {
            reasoning_pieces.push(reasoning.as_str());
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

    let completion = crate::provider::sse::aggregate_stream_events(events, "chatgpt").unwrap();
    assert_eq!(completion.usage.total_tokens, 30);
    assert_eq!(completion.choice.len(), 1);
    if let AssistantContent::Reasoning(r) = &completion.choice[0] {
        if let rig::message::ReasoningContent::Text { text, .. } = &r.content[0] {
            assert_eq!(
                text,
                "Thinking about the plan.\n\nStep 1: Check code.\n\nStep 2: Run tests."
            );
        } else {
            panic!("expected Text reasoning content");
        }
    } else {
        panic!("expected Reasoning choice");
    }
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

    let completion = crate::provider::sse::aggregate_stream_events(events, "chatgpt").unwrap();
    assert_eq!(completion.usage.total_tokens, 65);
    assert_eq!(completion.choice.len(), 2);
    assert!(matches!(&completion.choice[0], AssistantContent::Text(t) if t.text == "Let me run that."));
    if let AssistantContent::ToolCall(tc) = &completion.choice[1] {
        assert_eq!(tc.function.name, "bash");
        assert_eq!(tc.function.arguments, serde_json::json!({"command": "cargo check"}));
        assert_eq!(tc.id, "call_1");
    } else {
        panic!("expected ToolCall");
    }
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
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"output_index\": 1, \"summary_index\": 1, \"delta\": \"**Designing workflow reconciliation for stale tasks**\"}\n\n",
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
        if let Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) = event {
            reasoning_pieces.push(reasoning.as_str());
        }
    }

    let full_reasoning = reasoning_pieces.concat();
    assert!(!full_reasoning.contains("****"));
    assert_eq!(
        full_reasoning,
        "**Designing durable task reconciliation**\n\n\
         **Planning startup task refresher invocation**\n\n\
         **Evaluating job schedule frequency trade-offs**\n\n\
         **Implementing task reconciliation at startup**\n\n\
         **Designing workflow reconciliation for stale tasks**"
    );
}

#[test]
fn reasoning_summary_without_output_index_splits_resetting_indices() {
    let mut parser = SseParser::new();

    let chunks = [
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"summary_index\": 2, \"delta\": \"**Part A**\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary_text.delta\", \"summary_index\": 0, \"delta\": \"**Part B**\"}\n\n",
    ];

    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk.as_bytes()));
    }

    let pieces: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) => Some(reasoning.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(pieces, vec!["**Part A**", "\n\n", "**Part B**"]);
}

#[test]
fn reasoning_summary_delta_synonym_events_split_parts() {
    let mut parser = SseParser::new();

    let chunks = [
        "data: {\"type\": \"response.reasoning_summary.delta\", \"output_index\": 0, \"summary_index\": 0, \"delta\": \"First phase\"}\n\n",
        "data: {\"type\": \"response.reasoning_summary.delta\", \"output_index\": 0, \"summary_index\": 1, \"delta\": \"Second phase\"}\n\n",
    ];

    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk.as_bytes()));
    }

    let pieces: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) => Some(reasoning.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(pieces, vec!["First phase", "\n\n", "Second phase"]);
}
