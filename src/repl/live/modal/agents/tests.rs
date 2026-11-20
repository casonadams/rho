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
    assert!(
        handle_agents_key(&mut c, key(KeyCode::Enter), &monitor)
            .unwrap()
            .consumed()
    );
    assert_eq!(active_title(&c).as_deref(), Some("Agent: scan #2 · scout"));
    let body = crate::ui::block::strip_ansi(&c.state().active_modal().unwrap().body);
    assert!(body.contains("Scan crates/"), "{body}");
    assert!(body.contains("⟳ rg {\"pattern\": \"unwrap\"}"), "{body}");

    assert!(
        handle_agents_key(&mut c, key(KeyCode::Left), &monitor)
            .unwrap()
            .consumed()
    );
    assert_eq!(active_title(&c).as_deref(), Some(AGENTS_TITLE));
    assert!(
        handle_agents_key(&mut c, key(KeyCode::Left), &monitor)
            .unwrap()
            .consumed()
    );
    assert!(c.state().active_modal().is_none());
    assert!(
        !handle_agents_key(&mut c, key(KeyCode::Left), &monitor)
            .unwrap()
            .consumed()
    );
}

fn type_text(c: &mut TerminalController<NullBackend>, monitor: &AgentMonitor, text: &str) {
    for ch in text.chars() {
        assert_eq!(
            handle_agents_key(c, key(KeyCode::Char(ch)), monitor).unwrap(),
            AgentKey::Handled
        );
    }
}

#[test]
fn the_view_is_full_screen_and_typing_in_it_sends_a_message_for_the_agents_task() {
    let (monitor, done, running) = monitor_with_two_agents();
    running.set_task("scan2-abcd");
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap();
    assert!(c.state().active_modal().unwrap().full_screen);

    type_text(&mut c, &monitor, "jk also check tests");
    assert_eq!(
        c.state().active_modal().unwrap().body_scroll,
        0,
        "j and k type instead of scrolling"
    );
    handle_agents_key(&mut c, key(KeyCode::Left), &monitor).unwrap();
    assert_eq!(
        active_title(&c).as_deref(),
        Some("Agent: scan #2 · scout"),
        "Left moves the cursor while typing"
    );

    let sent = handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap();
    assert_eq!(
        sent,
        AgentKey::Send(AgentMessage {
            task_id: "scan2-abcd".into(),
            label: "scan #2 · scout".into(),
            running: true,
            text: "jk also check tests".into(),
        })
    );
    assert!(c.state().active_modal().unwrap().input.text().is_empty());

    handle_agents_key(&mut c, key(KeyCode::Tab), &monitor).unwrap();
    assert_eq!(
        c.state().active_modal().unwrap().subject.as_deref(),
        Some(done.id().to_string().as_str())
    );
    type_text(&mut c, &monitor, "hi");
    assert_eq!(
        handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap(),
        AgentKey::Handled
    );
    assert!(
        c.state().system_message().is_some_and(|m| m.contains("not saved")),
        "a round without a task cannot be messaged"
    );
}

#[test]
fn ctrl_c_clears_a_typed_message_before_closing_the_view() {
    let (monitor, _, _) = monitor_with_two_agents();
    let mut c = controller();
    try_open_agents(&mut c, &key(KeyCode::Left), &monitor).unwrap();
    handle_agents_key(&mut c, key(KeyCode::Enter), &monitor).unwrap();
    type_text(&mut c, &monitor, "draft");
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

    handle_agents_key(&mut c, ctrl_c, &monitor).unwrap();
    assert!(c.state().active_modal().unwrap().input.text().is_empty());
    assert!(c.state().active_modal().is_some_and(is_agent_view));
    handle_agents_key(&mut c, ctrl_c, &monitor).unwrap();
    assert_eq!(active_title(&c).as_deref(), Some(AGENTS_TITLE));
}

#[test]
fn the_view_follows_the_agent_into_the_round_that_continues_its_task() {
    let (monitor, done, _) = monitor_with_two_agents();
    let mut c = controller();
    let view = agent_view(&c, &monitor.record(done.id()).unwrap());
    c.state_mut().push_modal(view);
    let other = monitor.start("refine › round 1", "other · scout", "m", 5);
    refresh_agent_modals(&mut c, &monitor);
    assert_eq!(
        c.state().active_modal().unwrap().subject.as_deref(),
        Some(done.id().to_string().as_str()),
        "another agent's round is not followed"
    );

    let next = monitor.start("refine › round 1", "scan #1 · scout", "m", 5);
    next.event(AgentEvent::Task("Follow-up from the user".into()));
    assert!(refresh_agent_modals(&mut c, &monitor));

    let modal = c.state().active_modal().unwrap();
    assert_eq!(modal.subject.as_deref(), Some(next.id().to_string().as_str()));
    assert!(crate::ui::block::strip_ansi(&modal.body).contains("Follow-up from the user"));
    assert_ne!(modal.subject.as_deref(), Some(other.id().to_string().as_str()));
}

#[test]
fn a_message_for_a_finished_agent_asks_the_main_agent_to_continue_its_task() {
    let message = AgentMessage {
        task_id: "audit-1234".into(),
        label: "audit · critic".into(),
        running: false,
        text: "Also check the error path".into(),
    };
    let prompt = continue_prompt(&message);
    assert!(
        prompt.starts_with("Continue subagent task `audit-1234` (audit · critic) with the subagent tool"),
        "{prompt}"
    );
    assert!(prompt.ends_with("\n\nAlso check the error path"), "{prompt}");
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

    let view = agent_view(&c, &monitor.record(running.id()).unwrap());
    c.state_mut().push_modal(view);
    late.finish(AgentStatus::Done);
    assert!(
        !refresh_agent_modals(&mut c, &monitor),
        "other agents do not change the view"
    );
    running.event(AgentEvent::Text("found it".into()));
    assert!(refresh_agent_modals(&mut c, &monitor));
    let body = crate::ui::block::strip_ansi(&c.state().active_modal().unwrap().body);
    assert!(body.trim_end().ends_with("found it"), "{body}");
}

fn tool(name: &str, output: &str, is_error: bool) -> rho_harness_core::presentation::ToolLine {
    rho_harness_core::presentation::ToolLine {
        name: name.into(),
        arguments: serde_json::json!({ "path": "src/parser.rs" }),
        is_error,
        output: output.into(),
        output_summary: output.lines().next().unwrap_or_default().into(),
        duration_ms: Some(12),
        file_change: None,
    }
}

#[test]
fn the_view_is_built_from_the_same_items_as_the_main_transcript() {
    let monitor = AgentMonitor::default();
    let handle = monitor.start("g", "a", "local/qwen", 5);
    handle.set_task("audit-1234");
    handle.event(AgentEvent::Task("Audit the parser".into()));
    handle.event(AgentEvent::Text("Looking at the parser".into()));
    handle.event(AgentEvent::Tool(tool("read", "1\n2", false)));
    handle.event(AgentEvent::ToolCall {
        name: "bash".into(),
        args: "{\"command\": \"make\"}".into(),
    });
    handle.turn(3);
    handle.finish(AgentStatus::Failed);
    let mut record = monitor.record(handle.id()).unwrap();
    record.dropped_events = 2;

    assert_eq!(
        agent_items(&record),
        [
            TranscriptItem::Notice("✗ failed · local/qwen · failed after 3 turns · 0s · g · task audit-1234".into()),
            TranscriptItem::Notice("(2 earlier events not kept)".into()),
            TranscriptItem::UserMessage("Audit the parser".into()),
            TranscriptItem::AssistantText("Looking at the parser".into()),
            TranscriptItem::Tool(tool("read", "1\n2", false)),
            TranscriptItem::Notice("⟳ bash {\"command\": \"make\"}".into()),
        ]
    );
    let body = crate::ui::block::strip_ansi(&render_agent_body(&controller(), &record));
    for expected in ["Audit the parser", "Looking at the parser", "read", "src/parser.rs"] {
        assert!(body.contains(expected), "{expected}: {body}");
    }
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
