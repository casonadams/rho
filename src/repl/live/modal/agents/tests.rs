use super::*;
use crate::ui::interactive::{InteractiveState, NullBackend, TranscriptItem};
use crossterm::event::KeyModifiers;
use rho_harness_core::agents::{AgentEvent, AgentHandle};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn controller() -> TerminalController<NullBackend> {
    TerminalController::new(NullBackend, InteractiveState::default()).unwrap()
}

fn monitor_with_two_agents() -> (AgentMonitor, AgentHandle, AgentHandle) {
    let monitor = AgentMonitor::default();
    let done = monitor.start("refine › round 1", "scan #1 · scout", "claude/claude-haiku-5-5", 12);
    done.event(AgentEvent::Task("Scan src/".into()));
    done.turn(2);
    done.finish(AgentStatus::Done);
    let running = monitor.start("refine › round 1", "scan #2 · scout", "claude/claude-haiku-5-5", 12);
    running.event(AgentEvent::Task("Scan crates/".into()));
    running.event(AgentEvent::tool_call("rg", serde_json::json!({ "pattern": "unwrap" })));
    running.turn(1);
    (monitor, done, running)
}

#[test]
fn left_on_an_empty_prompt_opens_the_list_with_running_agents_first() {
    let (monitor, _, running) = monitor_with_two_agents();
    let mut c = controller();
    assert!(try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap());
    let modal = c.state().active_modal().unwrap();
    assert_eq!(modal.title, AGENTS_TITLE);
    assert_eq!(
        modal.options[0].value.as_deref(),
        Some(running.id().to_string().as_str())
    );
    assert!(modal.options[0].label.starts_with("⟳ scan #2 · scout"));
    assert_eq!(
        modal.options[1].description.as_deref(),
        Some("refine › round 1 · claude/claude-haiku-5-5 · done after 2 turns · 0s")
    );
}

#[test]
fn left_is_left_alone_when_typing_or_when_a_modal_is_open() {
    let (monitor, _, _) = monitor_with_two_agents();
    let mut c = controller();
    c.state_mut().editor_mut().set_text("draft");
    assert!(!try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap());
    c.state_mut().editor_mut().set_text("");
    assert!(!try_open_agents(&mut c, &KeyEvent::new(KeyCode::Left, KeyModifiers::ALT), &monitor).unwrap());
    c.state_mut().push_modal(ModalState::new("Help", "", Vec::new()));
    assert!(!try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap());
}

#[test]
fn empty_monitor_explains_instead_of_opening() {
    let mut c = controller();
    assert!(try_open_agents(&mut c, &key(KeyCode::Left), &AgentMonitor::default()).unwrap());
    assert!(c.state().active_modal().is_none());
    assert!(
        c.state()
            .system_message()
            .is_some_and(|m| m.contains("No subagents yet"))
    );
}

#[test]
fn enter_shows_the_agent_in_the_main_terminal_and_left_returns_to_the_main_agent() {
    let (monitor, _, running) = monitor_with_two_agents();
    let mut c = controller();
    c.push_transcript_item(TranscriptItem::UserMessage("main prompt".into()))
        .unwrap();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();

    assert!(
        handle_agents_key(&mut c, key(KeyCode::Enter), &monitor)
            .unwrap()
            .consumed()
    );

    assert!(c.state().active_modal().is_none(), "the list closes");
    assert_eq!(c.viewed_agent(), Some(running.id()));
    assert_eq!(c.transcript(), [TranscriptItem::UserMessage("Scan crates/".into())]);
    let viewing = c.state().footer().viewing.clone().unwrap();
    assert!(
        viewing.starts_with("scan #2 · scout · claude/claude-haiku-5-5 · turn 1/12"),
        "{viewing}"
    );

    assert!(try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap());
    assert_eq!(c.viewed_agent(), None);
    assert_eq!(c.transcript(), [TranscriptItem::UserMessage("main prompt".into())]);
    assert_eq!(c.state().footer().viewing, None);
}

#[test]
fn esc_while_viewing_stops_a_running_agent_or_returns_from_a_finished_one() {
    let (monitor, done, running) = monitor_with_two_agents();
    running.set_task("scan2-abcd");
    let mut c = controller();
    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Esc), &monitor).unwrap(),
        AgentKey::Ignored,
        "Esc is the main agent's when no agent is shown"
    );

    crate::repl::live::agent_focus::show_agent(&mut c, Some(&monitor), Some(running.id())).unwrap();
    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Esc), &monitor).unwrap(),
        AgentKey::Escape(Some(StopRequest {
            task_id: "scan2-abcd".into(),
            label: "scan #2 · scout".into(),
        }))
    );
    crate::repl::live::agent_focus::show_agent(&mut c, Some(&monitor), Some(done.id())).unwrap();
    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Esc), &monitor).unwrap(),
        AgentKey::Escape(None)
    );
}

#[test]
fn delete_in_the_list_asks_to_stop_the_selected_running_agent() {
    let (monitor, _, running) = monitor_with_two_agents();
    running.set_task("scan2-abcd");
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();

    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Delete), &monitor).unwrap(),
        AgentKey::Stop(StopRequest {
            task_id: "scan2-abcd".into(),
            label: "scan #2 · scout".into()
        })
    );
    handle_agents_key(&mut c, key(KeyCode::Down), &monitor).unwrap();
    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Delete), &monitor).unwrap(),
        AgentKey::Handled
    );
    assert!(c.state().system_message().is_some_and(|m| m.contains("is not running")));
}

#[test]
fn refresh_tracks_new_agents_and_keeps_the_selection() {
    let (monitor, done, _) = monitor_with_two_agents();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Down), &monitor).unwrap();
    let _late = monitor.start("refine › round 2", "validate · validator", "claude/claude-opus-5-5", 16);

    assert!(refresh_agent_modals(&mut c, &monitor).unwrap());

    let list = c.state().active_modal().unwrap();
    assert_eq!(list.options.len(), 3);
    assert_eq!(
        list.selected_option().unwrap().value.as_deref(),
        Some(done.id().to_string().as_str())
    );
    assert!(!refresh_agent_modals(&mut c, &monitor).unwrap());
    assert_eq!(c.state().footer().running_agents, 2);
}

#[test]
fn an_agent_waiting_for_approval_says_so_in_the_list() {
    let (monitor, _, running) = monitor_with_two_agents();
    let _waiting = running.awaiting_approval();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    let description = c.state().active_modal().unwrap().options[0]
        .description
        .clone()
        .unwrap();
    assert!(description.contains("turn 1/12 · needs approval"), "{description}");
}
