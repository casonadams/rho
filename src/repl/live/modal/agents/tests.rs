use super::*;
use crate::ui::interactive::{InteractiveState, NullBackend};
use rho_harness_core::agents::AgentHandle;

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
    running.event(AgentEvent::ToolCall {
        name: "rg".into(),
        args: "{\"pattern\": \"unwrap\"}".into(),
    });
    running.turn(1);
    (monitor, done, running)
}

fn active_title(controller: &TerminalController<NullBackend>) -> Option<String> {
    controller.state().active_modal().map(|m| m.title.clone())
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
fn enter_opens_a_view_and_left_walks_back_to_the_prompt() {
    let (monitor, _, _) = monitor_with_two_agents();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    assert!(handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap());
    assert_eq!(active_title(&c).as_deref(), Some("Agent: scan #2 · scout"));
    let body = &c.state().active_modal().unwrap().body;
    assert!(body.contains("▍Task\n  Scan crates/"), "{body}");
    assert!(body.contains("→ rg {\"pattern\": \"unwrap\"}"), "{body}");

    assert!(handle_agents_key(&mut c, key(KeyCode::Left), &monitor).unwrap());
    assert_eq!(active_title(&c).as_deref(), Some(AGENTS_TITLE));
    assert!(handle_agents_key(&mut c, key(KeyCode::Left), &monitor).unwrap());
    assert!(c.state().active_modal().is_none());
    assert!(!handle_agents_key(&mut c, key(KeyCode::Left), &monitor).unwrap());
}

#[test]
fn tab_in_a_view_jumps_to_the_next_agent() {
    let (monitor, done, _) = monitor_with_two_agents();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Tab), &monitor).unwrap();
    let modal = c.state().active_modal().unwrap();
    assert_eq!(modal.subject.as_deref(), Some(done.id().to_string().as_str()));
    assert_eq!(c.state().modal_depth(), 2);
}

#[test]
fn refresh_tracks_new_agents_and_follows_the_viewed_agent() {
    let (monitor, done, running) = monitor_with_two_agents();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Down), &monitor).unwrap();
    let late = monitor.start("refine › round 2", "validate · validator", "claude/claude-opus-5-5", 16);
    assert!(refresh_agent_modals(&mut c, &monitor));
    let list = c.state().active_modal().unwrap();
    assert_eq!(list.options.len(), 3);
    assert_eq!(
        list.selected_option().unwrap().value.as_deref(),
        Some(done.id().to_string().as_str())
    );
    assert!(!refresh_agent_modals(&mut c, &monitor));

    c.state_mut()
        .push_modal(agent_view(&monitor.record(running.id()).unwrap()));
    late.finish(AgentStatus::Done);
    assert!(
        !refresh_agent_modals(&mut c, &monitor),
        "other agents do not change the view"
    );
    running.event(AgentEvent::Text("found it".into()));
    assert!(refresh_agent_modals(&mut c, &monitor));
    assert!(c.state().active_modal().unwrap().body.ends_with("● found it"));
}

#[test]
fn body_shows_status_tool_results_and_dropped_events() {
    let monitor = AgentMonitor::default();
    let handle = monitor.start("g", "a", "local/qwen", 5);
    handle.event(AgentEvent::Text("Looking at the parser".into()));
    handle.event(AgentEvent::ToolResult {
        name: "read".into(),
        output: "1\n2\n3\n4\n5\n6".into(),
        is_error: false,
    });
    handle.event(AgentEvent::ToolResult {
        name: "bash".into(),
        output: "boom".into(),
        is_error: true,
    });
    handle.turn(3);
    handle.finish(AgentStatus::Failed);
    let mut record = monitor.record(handle.id()).unwrap();
    record.dropped_events = 2;
    let body = render_agent_body(&record);
    assert!(
        body.starts_with("✗ failed · local/qwen · failed after 3 turns · 0s · g"),
        "{body}"
    );
    assert!(body.contains("(2 earlier events not kept)"));
    assert!(body.contains("● Looking at the parser"));
    assert!(
        body.contains("  ← read\n    1\n    2\n    3\n    4\n    … 2 more lines"),
        "{body}"
    );
    assert!(body.contains("  ✗ bash\n    boom"));
}

#[test]
fn view_scrolling_moves_away_from_and_back_to_the_tail() {
    let mut modal = ModalState::new("Agent: a", "1\n2\n3\n4\n5", Vec::new()).with_body_anchor(BodyAnchor::Bottom);
    scroll_view(&mut modal, KeyCode::Up);
    scroll_view(&mut modal, KeyCode::PageUp);
    assert_eq!(modal.body_scroll, 5);
    scroll_view(&mut modal, KeyCode::Down);
    assert_eq!(modal.body_scroll, 4);
    scroll_view(&mut modal, KeyCode::End);
    assert_eq!(modal.body_scroll, 0);
}
