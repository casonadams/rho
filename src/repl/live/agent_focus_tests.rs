use super::*;
use crate::repl::live::batch::LiveBatch;
use crate::ui::interactive::{
    InteractionOption, InteractionPrompt, InteractionResponder, InteractiveState, NullBackend, OptionLayout, UiEvent,
};
use rho_harness_core::agents::AgentHandle;
use rho_harness_core::config::ApprovalFocus;

fn controller() -> TerminalController<NullBackend> {
    TerminalController::new(NullBackend, InteractiveState::default()).unwrap()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn running_agent(monitor: &AgentMonitor, label: &str) -> AgentHandle {
    let handle = monitor.start("subagents", label, "acme/smol", 10);
    handle.event(AgentEvent::Task("Audit the parser".into()));
    handle
}

fn tool_line(name: &str) -> rho_harness_core::presentation::ToolLine {
    rho_harness_core::presentation::ToolLine {
        name: name.into(),
        arguments: serde_json::json!({ "command": "make test" }),
        is_error: false,
        output: "ok".into(),
        output_summary: "ok".into(),
        duration_ms: Some(5),
        file_change: None,
    }
}

#[test]
fn a_running_agent_streams_its_reply_and_shows_its_running_tool_like_the_main_agent() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "audit");
    handle.event(AgentEvent::ToolCall {
        name: "bash".into(),
        args: serde_json::json!({ "command": "make test" }).to_string(),
    });
    let with_tool = frame(&monitor.record(handle.id()).unwrap());
    assert_eq!(
        with_tool.items,
        [TranscriptItem::UserMessage("Audit the parser".into())]
    );
    let tool = with_tool.running_tool.expect("bash gets a live card");
    assert_eq!(tool.name, "bash");
    assert!(tool.args_summary.contains("make test"), "{}", tool.args_summary);

    handle.event(AgentEvent::Tool(tool_line("bash")));
    handle.event(AgentEvent::Text("Tests pass".into()));
    let streaming = frame(&monitor.record(handle.id()).unwrap());
    assert_eq!(streaming.items.len(), 2, "{:?}", streaming.items);
    assert_eq!(streaming.streaming.as_deref(), Some("Tests pass"));
    assert_eq!(streaming.running_tool, None);

    handle.finish(AgentStatus::Done);
    let settled = frame(&monitor.record(handle.id()).unwrap());
    assert_eq!(
        settled.items.last(),
        Some(&TranscriptItem::AssistantText("Tests pass".into()))
    );
    assert_eq!(settled.streaming, None);
}

#[test]
fn tools_without_a_live_card_and_unfinished_calls_follow_the_main_agents_rules() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "scout");
    handle.event(AgentEvent::ToolCall {
        name: "rg".into(),
        args: "{\"pattern\": \"x\"}".into(),
    });
    assert_eq!(frame(&monitor.record(handle.id()).unwrap()).running_tool, None);

    handle.finish(AgentStatus::Cancelled);
    let stopped = frame(&monitor.record(handle.id()).unwrap());
    assert_eq!(
        stopped.items.last(),
        Some(&TranscriptItem::Notice("\u{2717} rg did not finish".into()))
    );
}

#[test]
fn the_shown_agents_reply_streams_in_place_and_settles_into_the_transcript() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "audit");
    let mut c = controller();
    show_agent(&mut c, Some(&monitor), Some(handle.id())).unwrap();
    handle.stream(rho_harness_core::agents::Streamed::Text, "Look");
    assert!(refresh(&mut c, &monitor).unwrap());
    handle.stream(rho_harness_core::agents::Streamed::Text, "ing closer");
    assert!(refresh(&mut c, &monitor).unwrap());
    assert_eq!(c.transcript().len(), 1, "a reply being written is not settled yet");

    handle.finish_text("Looking closer");
    handle.finish(AgentStatus::Done);
    refresh(&mut c, &monitor).unwrap();
    assert_eq!(
        c.transcript().last(),
        Some(&TranscriptItem::AssistantText("Looking closer".into()))
    );
    assert_eq!(c.transcript().len(), 2);
}

#[test]
fn the_main_agents_output_is_held_while_an_agent_is_shown_and_replayed_on_return() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "audit");
    let mut c = controller();
    let mut batch = LiveBatch::turn();
    c.push_transcript_item(TranscriptItem::UserMessage("main prompt".into()))
        .unwrap();
    show_agent(&mut c, Some(&monitor), Some(handle.id())).unwrap();

    batch
        .enqueue(
            &mut c,
            UiEvent::Transcript(TranscriptItem::Notice("main kept working".into())),
        )
        .unwrap();
    assert_eq!(c.transcript(), [TranscriptItem::UserMessage("Audit the parser".into())]);

    show_agent(&mut c, Some(&monitor), None).unwrap();
    assert_eq!(
        c.transcript(),
        [
            TranscriptItem::UserMessage("main prompt".into()),
            TranscriptItem::Notice("main kept working".into()),
        ]
    );
}

#[test]
fn the_view_follows_the_agent_into_the_round_that_continues_its_task() {
    let monitor = AgentMonitor::default();
    let first = running_agent(&monitor, "audit");
    first.finish(AgentStatus::Done);
    let mut c = controller();
    show_agent(&mut c, Some(&monitor), Some(first.id())).unwrap();
    let other = running_agent(&monitor, "other");
    refresh(&mut c, &monitor).unwrap();
    assert_eq!(c.viewed_agent(), Some(first.id()), "another agent is not followed");

    let next = monitor.start("subagents", "audit", "acme/smol", 10);
    next.event(AgentEvent::Task("Also check tests".into()));
    assert!(refresh(&mut c, &monitor).unwrap());

    assert_eq!(c.viewed_agent(), Some(next.id()));
    assert_ne!(c.viewed_agent(), Some(other.id()));
    assert_eq!(c.transcript(), [TranscriptItem::UserMessage("Also check tests".into())]);
}

#[test]
fn the_shown_agent_is_resynced_only_when_the_monitor_changes() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "audit");
    let mut c = controller();
    show_agent(&mut c, Some(&monitor), Some(handle.id())).unwrap();
    assert_eq!(c.view_synced_version(), None);

    assert!(!refresh(&mut c, &monitor).unwrap());
    assert_eq!(c.view_synced_version(), Some(monitor.version()));
    assert!(!refresh(&mut c, &monitor).unwrap());

    handle.stream(rho_harness_core::agents::Streamed::Text, "Look");
    assert_ne!(c.view_synced_version(), Some(monitor.version()));
    assert!(refresh(&mut c, &monitor).unwrap());
    assert_eq!(c.view_synced_version(), Some(monitor.version()));
}

#[test]
fn enter_while_an_agent_is_shown_takes_the_prompt_for_that_agent() {
    let monitor = AgentMonitor::default();
    let handle = running_agent(&monitor, "audit");
    let mut c = controller();
    c.state_mut().editor_mut().set_text("also check tests");
    assert_eq!(
        take_submission(&mut c, &key(KeyCode::Enter)),
        None,
        "not while the main agent is shown"
    );

    show_agent(&mut c, Some(&monitor), Some(handle.id())).unwrap();
    let shifted = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(take_submission(&mut c, &shifted), None, "Shift+Enter adds a line");
    assert_eq!(
        take_submission(&mut c, &key(KeyCode::Enter)).as_deref(),
        Some("also check tests")
    );
    assert!(c.state().editor().text().is_empty());
    assert_eq!(take_submission(&mut c, &key(KeyCode::Enter)), None, "nothing typed");
}

async fn engine() -> (crate::engine::AgentEngine, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = rho_harness_core::config::Config {
        provider: "local".to_string(),
        model: "llama3.2".to_string(),
        ..crate::repl::live::modal::selectors::test_support::test_config(temp.path())
    };
    let engine = crate::engine::builder::AgentEngineBuilder::new(config, rho_engine::auth::AuthStore::default())
        .build()
        .await
        .unwrap();
    (engine, temp)
}

#[tokio::test]
async fn a_message_to_a_finished_agent_asks_the_main_agent_to_continue_its_task() {
    let (engine, _temp) = engine().await;
    let monitor = engine.agent_monitor();
    let saved = running_agent(monitor, "audit");
    saved.set_task("audit-1234");
    saved.finish(AgentStatus::Done);
    let unsaved = running_agent(monitor, "scratch");
    unsaved.finish(AgentStatus::Done);
    let mut c = controller();

    show_agent(&mut c, Some(monitor), Some(saved.id())).unwrap();
    assert_eq!(route(&mut c, &engine, "/model"), Routed::Handled);
    assert!(
        c.state()
            .system_message()
            .is_some_and(|m| m.contains("Commands act on the main session"))
    );
    let Routed::ContinueThroughMain(prompt) = route(&mut c, &engine, "Also check the error path") else {
        panic!("a finished agent is continued through the main agent");
    };
    assert!(
        prompt.starts_with("Continue subagent task `audit-1234` (audit) with the subagent tool"),
        "{prompt}"
    );
    assert!(prompt.ends_with("\n\nAlso check the error path"), "{prompt}");

    show_agent(&mut c, Some(monitor), Some(unsaved.id())).unwrap();
    assert_eq!(route(&mut c, &engine, "hi"), Routed::Handled);
    assert!(c.state().system_message().is_some_and(|m| m.contains("not saved")));
}

fn approval() -> UiEvent {
    let (responder, _) = tokio::sync::oneshot::channel();
    UiEvent::Interaction {
        prompt: InteractionPrompt {
            title: "Permission required".into(),
            body: "bash make".into(),
            options: vec![InteractionOption {
                label: "Allow".into(),
                description: None,
                input: None,
            }],
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: OptionLayout::Vertical,
        },
        responder: InteractionResponder { responder },
    }
}

#[test]
fn an_agents_approval_switches_the_view_to_it_by_default() {
    let monitor = AgentMonitor::default();
    let asker = running_agent(&monitor, "builder");
    let mut c = controller();
    let mut batch = LiveBatch::turn().with_monitor(monitor.clone());
    let _waiting = asker.awaiting_approval();

    batch.enqueue(&mut c, approval()).unwrap();

    assert_eq!(c.viewed_agent(), Some(asker.id()));
    assert_eq!(
        c.state().active_modal().map(|m| m.title.as_str()),
        Some("Permission required")
    );
}

#[test]
fn the_main_agents_approval_switches_back_from_a_shown_agent() {
    let monitor = AgentMonitor::default();
    let shown = running_agent(&monitor, "audit");
    let mut c = controller();
    let mut batch = LiveBatch::turn().with_monitor(monitor.clone());
    show_agent(&mut c, Some(&monitor), Some(shown.id())).unwrap();

    batch.enqueue(&mut c, approval()).unwrap();

    assert_eq!(c.viewed_agent(), None);
    assert!(c.state().active_modal().is_some());
}

#[test]
fn with_indicate_an_approval_waits_in_the_footer_until_its_agent_is_shown() {
    let monitor = AgentMonitor::default();
    let asker = running_agent(&monitor, "builder");
    let mut c = controller();
    c.set_approval_focus(ApprovalFocus::Indicate);
    let mut batch = LiveBatch::turn().with_monitor(monitor.clone());
    let _waiting = asker.awaiting_approval();

    batch.enqueue(&mut c, approval()).unwrap();

    assert_eq!(c.viewed_agent(), None, "the view stays on the main agent");
    assert!(c.state().active_modal().is_none());
    assert_eq!(c.state().footer().awaiting_approval.as_deref(), Some("builder"));

    show_agent(&mut c, Some(&monitor), Some(asker.id())).unwrap();
    batch.flush(&mut c, false).unwrap();
    assert_eq!(
        c.state().active_modal().map(|m| m.title.as_str()),
        Some("Permission required")
    );
    assert_eq!(c.state().footer().awaiting_approval, None);
}

#[test]
fn a_held_approval_that_is_withdrawn_clears_its_indicator() {
    let monitor = AgentMonitor::default();
    let asker = running_agent(&monitor, "builder");
    let mut c = controller();
    c.set_approval_focus(ApprovalFocus::Indicate);
    let mut batch = LiveBatch::turn().with_monitor(monitor.clone());
    let waiting = asker.awaiting_approval();
    batch.enqueue(&mut c, approval()).unwrap();
    drop(waiting);

    batch.enqueue(&mut c, UiEvent::DismissInteraction).unwrap();

    assert_eq!(c.state().footer().awaiting_approval, None);
    show_agent(&mut c, Some(&monitor), Some(asker.id())).unwrap();
    batch.flush(&mut c, false).unwrap();
    assert!(c.state().active_modal().is_none());
}
