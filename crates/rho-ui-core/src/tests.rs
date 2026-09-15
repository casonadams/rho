use super::*;

#[test]
fn test_parse_markdown_headings_and_paragraphs() {
    let md = "# Title\n\nThis is a **bold** and *italic* test.\n\n## Subtitle\n\nAnother paragraph with `code`.";
    let blocks = parse_markdown(md);

    assert_eq!(blocks.len(), 4);
    match &blocks[0] {
        ContentBlock::Heading { level, content } => {
            assert_eq!(*level, 1);
            assert_eq!(content[0].plain_text(), "Title");
        }
        _ => panic!("expected heading"),
    }
    match &blocks[1] {
        ContentBlock::Paragraph(inlines) => {
            assert!(inlines.iter().any(|i| matches!(i, InlineSpan::Bold(_))));
            assert!(inlines.iter().any(|i| matches!(i, InlineSpan::Italic(_))));
        }
        _ => panic!("expected paragraph"),
    }
}

#[test]
fn test_parse_markdown_code_fences_and_mermaid() {
    let md = "```rust\nfn main() {}\n```\n\n```mermaid\ngraph TD;\nA-->B;\n```";
    let blocks = parse_markdown(md);

    assert_eq!(blocks.len(), 2);
    match &blocks[0] {
        ContentBlock::CodeFence { language, content, .. } => {
            assert_eq!(language, "rust");
            assert!(content.contains("fn main()"));
        }
        _ => panic!("expected code fence"),
    }
    match &blocks[1] {
        ContentBlock::Diagram { kind, content } => {
            assert_eq!(*kind, DiagramKind::Mermaid);
            assert!(content.contains("A-->B"));
        }
        _ => panic!("expected mermaid diagram"),
    }
}

#[test]
fn test_generate_diff_with_similar() {
    let old_text = "line 1\nline 2\nline 3\n";
    let new_text = "line 1\nmodified line 2\nline 3\nline 4\n";

    let diff_block = generate_diff(old_text, new_text, Some("test.rs".to_string()));
    match diff_block {
        ContentBlock::Diff { file_path, hunks, .. } => {
            assert_eq!(file_path.as_deref(), Some("test.rs"));
            assert!(hunks.iter().any(|h| h.change == ChangeType::Removed));
            assert!(hunks.iter().any(|h| h.change == ChangeType::Added));
            assert!(hunks.iter().any(|h| h.change == ChangeType::Equal));
        }
        _ => panic!("expected diff block"),
    }
}

#[test]
fn test_generate_word_diff() {
    let old_line = "let value = 1;";
    let new_line = "let value = 2;";

    let spans = generate_word_diff(old_line, new_line);
    assert!(spans.iter().any(|s| match s {
        InlineSpan::Styled {
            style: StyleToken::Error,
            text,
        } => text.contains('1'),
        _ => false,
    }));
    assert!(spans.iter().any(|s| match s {
        InlineSpan::Styled {
            style: StyleToken::Success,
            text,
        } => text.contains('2'),
        _ => false,
    }));
}

#[test]
fn test_stream_chunk_parser_thinking_split_across_chunks() {
    let mut parser = StreamChunkParser::new();
    let events1 = parser.parse_chunk("Hello <think");
    let events2 = parser.parse_chunk("ing>Analyzing the code...");
    let events3 = parser.parse_chunk(" done.</think");
    let events4 = parser.parse_chunk("ing> Now here is the answer.");
    let events5 = parser.flush();

    let all_events: Vec<StreamEvent> = [events1, events2, events3, events4, events5].concat();

    assert!(
        all_events
            .iter()
            .any(|e| matches!(e, StreamEvent::Token(t) if t == "Hello "))
    );
    assert!(all_events.iter().any(|e| matches!(e, StreamEvent::ThinkingStarted)));
    assert!(
        all_events
            .iter()
            .any(|e| matches!(e, StreamEvent::ThinkingDelta(d) if d.contains("Analyzing")))
    );
    assert!(
        all_events
            .iter()
            .any(|e| matches!(e, StreamEvent::ThinkingFinished { .. }))
    );
    assert!(
        all_events
            .iter()
            .any(|e| matches!(e, StreamEvent::Token(t) if t.contains("Now here")))
    );
}

#[test]
fn test_completion_engine_commands_and_arguments() {
    let mut engine = CompletionEngine::new();
    engine
        .models
        .push(("claude-3-5-sonnet".to_string(), "fast".to_string()));
    engine.models.push(("gpt-4o".to_string(), "openai".to_string()));
    engine.files.push("src/main.rs".to_string());

    let cmd_results = engine.complete("/mo", 3);
    assert!(!cmd_results.is_empty());
    assert_eq!(cmd_results[0].value, "/model");

    let arg_results = engine.complete("/model sonnet", 13);
    assert_eq!(arg_results.len(), 1);
    assert_eq!(arg_results[0].value, "/model claude-3-5-sonnet");

    let file_results = engine.complete("check @main", 11);
    assert_eq!(file_results.len(), 1);
    assert_eq!(file_results[0].value, "src/main.rs");
}

#[test]
fn test_prompt_history_navigation_and_draft_preservation() {
    let mut history = PromptHistory::new();
    history.record("first prompt");
    history.record("second prompt");

    assert_eq!(history.previous("draft"), Some("second prompt"));
    assert_eq!(history.previous("ignored"), Some("first prompt"));
    assert_eq!(history.next_entry(), Some("second prompt"));
    assert_eq!(history.next_entry(), Some("draft"));
}

#[test]
fn test_modal_state_navigation_filtering_and_jump_keys() {
    let options = vec![
        ModalOption {
            label: "Option Alpha".to_string(),
            description: Some("First entry".to_string()),
            value: "alpha".to_string(),
            is_active: false,
            shortcut: None,
        },
        ModalOption {
            label: "Option Beta".to_string(),
            description: Some("Second entry".to_string()),
            value: "beta".to_string(),
            is_active: true,
            shortcut: None,
        },
    ];
    let mut modal = ModalState::new("Test Modal", options);

    assert_eq!(modal.selected_option().unwrap().value, "alpha");
    modal.select_next();
    assert_eq!(modal.selected_option().unwrap().value, "beta");
    modal.select_prev();
    assert_eq!(modal.selected_option().unwrap().value, "alpha");

    let selected = modal.select_digit(2);
    assert_eq!(selected.unwrap().value, "beta");

    modal.set_filter("Alpha");
    assert_eq!(modal.filtered_options.len(), 1);
    assert_eq!(modal.filtered_options[0].value, "alpha");
}

#[test]
fn test_permission_prompt_actions() {
    let mut prompt = PermissionPromptState::new(
        "bash",
        "rm -rf /tmp/test",
        serde_json::json!({"command": "rm -rf /tmp/test"}),
    );

    assert_eq!(prompt.selected_index, 0);
    prompt.select_next();
    assert_eq!(prompt.selected_index, 1);

    prompt.start_editing();
    prompt.set_edited_command("rm -rf /tmp/safe");
    let action = prompt.resolve().unwrap();
    assert_eq!(
        action,
        PermissionAction::Edit {
            mutated_command: "rm -rf /tmp/safe".to_string()
        }
    );
}

#[test]
fn test_footer_metrics_formatting() {
    let metrics = FooterMetrics {
        input_tokens: 1_200,
        output_tokens: 450,
        cache_read_tokens: 10_000,
        cache_write_tokens: 2_000,
        context_tokens: 25_000,
        context_window: 200_000,
        total_cost: Some(0.015),
        tokens_per_second: Some(42.0),
        quota_summary: None,
    };

    let line = metrics.format_stats_line("claude-sonnet");
    assert!(line.contains("↑1.2k"));
    assert!(line.contains("↓450"));
    assert!(line.contains("R10.0k"));
    assert!(line.contains("W2.0k"));
    assert!(line.contains("$0.015"));
    assert!(line.contains("12.5%/200k"));
    assert!(line.contains("@42t/s"));
    assert!(line.contains("claude-sonnet"));
}

#[test]
fn test_rho_ticket_parsing() {
    let ticket = RhoTicket::parse("ticket:abc123node@https://relay.rho.dev").unwrap();
    assert_eq!(ticket.node_id, "abc123node");
    assert_eq!(ticket.relay_url.as_deref(), Some("https://relay.rho.dev"));
}

#[test]
fn test_secret_guard_redaction() {
    let exposed = "Using secret key sk-ant-api03-abcdef1234567890 in request";
    let redacted = SecretGuard::redact(exposed);
    assert!(!redacted.contains("abcdef1234567890"));
    assert!(redacted.contains("sk-ant-••••••••"));
}

#[test]
fn test_session_state_lifecycle_and_streaming() {
    let mut session = SessionState::new("sess-1", "claude-sonnet");
    assert_eq!(session.turn_state, SessionTurnState::Idle);

    session.handle_command(SessionCommand::Prompt {
        text: "Write a hello world program".to_string(),
    });
    assert!(matches!(session.turn_state, SessionTurnState::Running { .. }));

    session.apply_stream_event(StreamEvent::Token("Hello".to_string()));
    session.apply_stream_event(StreamEvent::Token(" world!".to_string()));
    session.apply_stream_event(StreamEvent::TurnCompleted);

    assert_eq!(session.turn_state, SessionTurnState::Idle);
    assert_eq!(session.blocks.len(), 2);
}
