//! Tests for the `ui::render` module.

use super::formatters::{
    format_bash_approval_card, format_edit_diff, format_session_status, format_thinking_block, format_write_preview,
};
use super::preview::{tool_title_style, webfetch_content_kind};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractionResponse, InteractiveUi, OutputEvent, UiEvent};
use crate::ui::theme::Theme;
use rho_core::bash_ast::RiskTier;
use rho_core::presentation::summary::{
    approval_heading, bash_approval_details, clean_command_paths, read_summary_parts, to_relative_path,
};
use rho_core::presentation::{ApprovalResult, BashApproval, SessionStatus, ToolLine};

#[test]
fn interactive_renderer_emits_formatted_output_and_activity_events() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    let activity = renderer.start_spinner("thinking...");
    renderer.print_thinking_token("considering");
    activity.finish_and_clear();
    renderer.print_token("answer");
    renderer.flush();
    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
        is_error: false,
        output: "contents".to_string(),
        output_summary: "contents".to_string(),
        duration_ms: None,
    });

    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                },
            )),
            UiEvent::RunningTool(_) | UiEvent::ToolStart(_) | UiEvent::ToolChunk { .. } | UiEvent::ToolEnd => {}
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    assert!(output.contains("considering"));
    assert!(output.contains("answer"));
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
}

#[test]
fn finished_bash_block_includes_elapsed_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "cargo test --all-targets"}),
        is_error: false,
        output: "test result: ok".to_string(),
        output_summary: "test result: ok".to_string(),
        duration_ms: Some(5000),
    });

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert!(output.contains("cargo test --all-targets"));
    assert!(output.contains("Took 5s"));
}

#[test]
fn print_session_status_and_notice_emit_transcript_item() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_session_status(&SessionStatus {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        context: "42% context".to_string(),
        quota: Some("80% quota".to_string()),
        auto_approve: true,
    });
    renderer.print_notice("  [Notice message]\n");

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("claude-sonnet"));
    assert!(items[0].contains("42% context"));
    assert!(items[1].contains("[Notice message]"));
}

#[tokio::test]
async fn interactive_tool_approval_supports_approve_and_denial_feedback() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let approval_renderer = renderer.clone();
    let approval = tokio::spawn(async move {
        approval_renderer
            .prompt_tool_approval("write", &serde_json::json!({"path": "out.txt", "content": "value"}))
            .await
    });
    let Some(UiEvent::Interaction { prompt, responder }) = events.recv().await else {
        panic!("expected approval request");
    };
    assert_eq!(prompt.title, "Approve write");
    assert_eq!(prompt.initial_selection, 0);
    responder.respond(InteractionResponse::Selected(0)).unwrap();
    assert_eq!(approval.await.unwrap(), ApprovalResult::Approved);

    let denial_renderer = renderer.clone();
    let denial = tokio::spawn(async move {
        denial_renderer
            .prompt_tool_approval("edit", &serde_json::json!({"path": "out.txt", "edits": []}))
            .await
    });
    let Some(UiEvent::Interaction { responder, .. }) = events.recv().await else {
        panic!("expected approval request");
    };
    responder.respond(InteractionResponse::Selected(1)).unwrap();
    let Some(UiEvent::Interaction { prompt, responder }) = events.recv().await else {
        panic!("expected feedback request");
    };
    assert!(prompt.allow_custom);
    responder
        .respond(InteractionResponse::Custom("use another file".to_string()))
        .unwrap();
    assert_eq!(
        denial.await.unwrap(),
        ApprovalResult::Denied {
            reason: "use another file".to_string()
        }
    );
}

#[tokio::test]
async fn interactive_bash_approval_preserves_session_and_high_risk_defaults() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let session_renderer = renderer.clone();
    let session_approval = tokio::spawn(async move {
        session_renderer
            .prompt_bash_approval(BashApproval {
                command: "cargo test".to_string(),
                tier: RiskTier::Mutating,
                reasons: Vec::new(),
            })
            .await
    });
    let Some(UiEvent::Interaction { prompt, responder }) = events.recv().await else {
        panic!("expected bash approval request");
    };
    assert_eq!(prompt.options.len(), 3);
    responder.respond(InteractionResponse::Selected(1)).unwrap();
    assert_eq!(session_approval.await.unwrap(), ApprovalResult::ApprovedForSession);

    let high_risk_renderer = renderer.clone();
    let high_risk = tokio::spawn(async move {
        let reasons = vec!["Discards changes".to_string()];
        high_risk_renderer
            .prompt_bash_approval(BashApproval {
                command: "git reset --hard".to_string(),
                tier: RiskTier::HighRisk,
                reasons: reasons.clone(),
            })
            .await
    });
    let Some(UiEvent::Interaction { prompt, responder }) = events.recv().await else {
        panic!("expected high-risk approval request");
    };
    assert_eq!(prompt.initial_selection, prompt.options.len() - 1);
    responder.respond(InteractionResponse::Cancelled).unwrap();
    assert_eq!(
        high_risk.await.unwrap(),
        ApprovalResult::Denied { reason: String::new() }
    );
}

#[tokio::test]
async fn interactive_budget_confirmation_fails_closed() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let continue_renderer = renderer.clone();
    let continuation = tokio::spawn(async move { continue_renderer.prompt_continue_budget(50).await });
    let Some(UiEvent::Interaction { responder, .. }) = events.recv().await else {
        panic!("expected budget request");
    };
    responder.respond(InteractionResponse::Selected(0)).unwrap();
    assert!(continuation.await.unwrap());

    let stop_renderer = renderer.clone();
    let stop = tokio::spawn(async move { stop_renderer.prompt_continue_budget(50).await });
    let Some(UiEvent::Interaction { responder, .. }) = events.recv().await else {
        panic!("expected budget request");
    };
    responder.respond(InteractionResponse::Cancelled).unwrap();
    assert!(!stop.await.unwrap());
}

#[test]
fn ordinary_bash_approval_shows_only_the_command() {
    let reasons = vec!["Writes output through file redirection".to_string()];
    let details = bash_approval_details(&BashApproval {
        command: "echo test > output.txt".to_string(),
        tier: RiskTier::Mutating,
        reasons: reasons.clone(),
    });

    assert_eq!(details, ["$ echo test > output.txt"]);
}

#[test]
fn high_risk_bash_approval_explains_the_risk() {
    let reasons = vec!["Discards uncommitted changes".to_string()];
    let details = bash_approval_details(&BashApproval {
        command: "git reset --hard".to_string(),
        tier: RiskTier::HighRisk,
        reasons: reasons.clone(),
    });

    assert_eq!(details, ["$ git reset --hard", "", "Discards uncommitted changes"]);
    assert_eq!(approval_heading(RiskTier::HighRisk), "High-risk bash command");
    assert_eq!(approval_heading(RiskTier::Mutating), "Bash command requires approval");
}

#[test]
fn bash_approval_cards_match_transcript_blocks() {
    let theme = Theme::default();
    let ordinary = format_bash_approval_card(
        &BashApproval {
            command: "cargo test".to_string(),
            tier: RiskTier::Mutating,
            reasons: Vec::new(),
        },
        &theme,
        40,
    );
    assert!(ordinary.contains("Bash command requires approval"));
    assert!(ordinary.contains("$ cargo test"));
    assert!(ordinary.contains("\x1b[33m"));

    let reasons = vec!["Discards uncommitted changes".to_string()];
    let high_risk = format_bash_approval_card(
        &BashApproval {
            command: "git reset --hard".to_string(),
            tier: RiskTier::HighRisk,
            reasons: reasons.clone(),
        },
        &theme,
        40,
    );
    assert!(high_risk.contains("High-risk bash command"));
    assert!(high_risk.contains("! Discards uncommitted changes"));
    assert!(high_risk.contains("\x1b[31m"));
}

#[test]
fn read_summaries_show_explicit_line_ranges() {
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20})),
        ("src/lib.rs".to_string(), Some(":10-29".to_string()))
    );
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs"})),
        ("src/lib.rs".to_string(), None)
    );
}

#[test]
fn test_to_relative_path() {
    let cwd = std::env::current_dir().unwrap();
    let abs = cwd.join("src/main.rs");
    let rel = to_relative_path(abs.to_str().unwrap());
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_clean_command_paths() {
    let cwd = std::env::current_dir().unwrap();
    let cwd_str = cwd.to_str().unwrap();
    let cmd = format!("cat {cwd_str}/Cargo.toml");
    let cleaned = clean_command_paths(&cmd);
    assert_eq!(cleaned, "cat Cargo.toml");
}

#[test]
fn test_format_edit_diff_renders_removals_and_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let x = 1;",
                "newText": "let x = 2;\nlet y = 3;"
            }
        ]
    });
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains("```diff"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
    assert!(diff.contains("```"));
    assert!(diff.ends_with('\n'));
}

#[test]
fn test_format_write_preview_renders_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme).unwrap();
    assert!(preview.contains("```diff"));
    assert!(preview.contains("+ def main():"));
    assert!(preview.contains("+     print('hello')"));
    assert!(preview.contains("```"));
}

#[test]
fn error_tool_titles_use_terminal_red_without_dimming() {
    assert_eq!(tool_title_style(false).render().to_string(), "\x1b[1m");
    assert_eq!(tool_title_style(true).render().to_string(), "\x1b[1m\x1b[31m");
}

#[test]
fn webfetch_content_kind_uses_format_or_url_extension() {
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/page"})),
        "text"
    );
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/data.json"})),
        "json"
    );
    assert_eq!(
        webfetch_content_kind(&serde_json::json!({"url": "https://example.com/file", "format": "pdf"})),
        "pdf"
    );
}

#[test]
fn session_status_keeps_runtime_context_visible() {
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            context: "27.4% (1M)".to_string(),
            quota: Some("93% (3h22m)".to_string()),
            auto_approve: false,
        }),
        "claude-sonnet | 27.4% (1M) | 93% (3h22m)"
    );
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "qwen".to_string(),
            provider: "ollama".to_string(),
            context: "0% (376k)".to_string(),
            quota: None,
            auto_approve: true,
        }),
        "qwen | 0% (376k)"
    );
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
    assert!(formatted.contains("analyzing the problem"));
    assert!(formatted.contains("checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with("\n\n"));
}
