use super::helpers::{final_event, presenter, request, terminal_session, test_engine};
use crate::approval::{ApprovalDecision, ApprovalEventSink, ApprovalRequest, ToolEvent};
use crate::bash_ast::RiskTier;
use crate::config::Config;
use crate::engine::runner::{TerminalApprovalSink, TerminalSinkConfig};
use crate::policy::ExecutionClass;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractionResponse, InteractiveUi, OutputEvent, UiEvent};
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[test]
fn approval_required_holds_spinner_until_granted() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "rm -rf target" }),
        class: ExecutionClass::ApprovalRequired {
            tier: RiskTier::HighRisk,
            reasons: vec!["destructive".to_string()],
        },
    });
    // No spinner while the approval prompt is on screen.
    assert!(sink.state.lock().unwrap().spinner.is_none());
    assert!(sink.state.lock().unwrap().pending.contains_key("call-1"));

    sink.emit(ToolEvent::ApprovalGranted {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
    });
    // The spinner starts only once execution is actually approved.
    assert!(sink.state.lock().unwrap().spinner.is_some());
}

#[test]
fn auto_approved_bash_call_surfaces_the_running_command_and_finishing_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: true,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "cargo test --all-targets" }),
        class: ExecutionClass::ReadOnly,
    });

    let tool_starts = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::ToolStart(req) => Some((req.name, req.args_summary)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_starts.len(), 1);
    assert_eq!(tool_starts[0].0, "bash");
    assert!(tool_starts[0].1.contains("cargo test --all-targets"));

    sink.emit(ToolEvent::Finished {
        internal_call_id: "call-1".to_string(),
        tool_name: "bash".to_string(),
        arguments: serde_json::json!({ "command": "cargo test --all-targets" }),
        output: "test result: ok".to_string(),
        status: "success".to_string(),
    });

    let mut tool_ended = false;
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::ToolEnd => tool_ended = true,
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
            | UiEvent::Todos(_)
            | UiEvent::Subagents(_) => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert!(tool_ended);
    assert!(output.contains("cargo test --all-targets"));
    assert!(output.contains("Took"));
}

#[tokio::test]
async fn interactive_approval_sink_awaits_typed_ui_response() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    let request_sink = sink.clone();
    let decision = tokio::spawn(async move {
        request_sink
            .request_approval(ApprovalRequest {
                tool_name: "write".to_string(),
                arguments: serde_json::json!({"path": "out.txt", "content": "value"}),
                tier: RiskTier::Mutating,
                reasons: vec!["writes a file".to_string()],
            })
            .await
    });

    let event = loop {
        let event = events.recv().await.unwrap();
        if matches!(event, UiEvent::Interaction { .. }) {
            break event;
        }
    };
    let UiEvent::Interaction { responder, .. } = event else {
        unreachable!();
    };
    responder.respond(InteractionResponse::Selected(0)).unwrap();
    assert_eq!(decision.await.unwrap(), ApprovalDecision::Approved);
}

#[test]
fn approval_denied_leaves_no_spinner() {
    let renderer = TerminalRenderer::default();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            auto_approve: false,
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        terminal_session(),
    );
    sink.emit(ToolEvent::CallClassified {
        internal_call_id: "call-1".to_string(),
        tool_name: "write".to_string(),
        arguments: serde_json::json!({ "path": "/tmp/x", "content": "y" }),
        class: ExecutionClass::ApprovalRequired {
            tier: RiskTier::Mutating,
            reasons: vec!["outside".to_string()],
        },
    });
    sink.emit(ToolEvent::ApprovalDenied {
        internal_call_id: "call-1".to_string(),
        tool_name: "write".to_string(),
    });
    assert!(sink.state.lock().unwrap().spinner.is_none());
}

#[tokio::test]
async fn session_approval_persists_across_multiple_engine_turns() {
    let (ui, mut events) = crate::ui::interactive::InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "bash", serde_json::json!({"command": "touch first.txt"})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("done 1"), final_event(Usage::new())],
        [
            MockStreamEvent::tool_call("call-2", "bash", serde_json::json!({"command": "touch second.txt"})),
            final_event(Usage::new()),
        ],
        [MockStreamEvent::text("done 2"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model, Config::default());

    let approvals = engine.session_approvals.clone();
    let turn1_engine = engine;
    let turn1_renderer = renderer.clone();
    let runner_turn_1 = tokio::spawn(async move {
        let output = turn1_engine
            .run_turn(request("turn 1"), presenter(&turn1_renderer))
            .await;
        (turn1_engine, output)
    });

    let event = loop {
        let event = events.recv().await.unwrap();
        if matches!(event, crate::ui::interactive::UiEvent::Interaction { .. }) {
            break event;
        }
    };
    let crate::ui::interactive::UiEvent::Interaction { responder, .. } = event else {
        unreachable!();
    };
    responder
        .respond(crate::ui::interactive::InteractionResponse::Selected(1))
        .unwrap();

    let (engine, output_1) = runner_turn_1.await.unwrap();
    assert_eq!(output_1.unwrap().tool_calls_count, 1);
    assert!(approvals.lock().unwrap().contains("touch *"));

    let output_2 = engine.run_turn(request("turn 2"), presenter(&renderer)).await.unwrap();
    assert_eq!(output_2.tool_calls_count, 1);
}
