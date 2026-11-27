use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::idle::LiveIdleContext;
use crate::repl::live::idle::dispatch::{
    IdleInputResult, RawInput, classify_event, handle_focus, handle_misc_action, handle_raw_paste, handle_resize,
    process_raw_input,
};
use crate::repl::live::types::EditorResources;
use crate::ui::interactive::NullBackend;
use crate::ui::interactive::{InputAction, QueueKind, QueuedMessage, TerminalController};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

async fn setup_idle_dispatch_harness(
    temp: &std::path::Path,
) -> (
    TerminalController<NullBackend>,
    InteractiveHistory,
    CompletionSet,
    LiveBatch,
    crate::repl::ReplSession,
    crate::engine::AgentEngine,
    crate::repl::input_reader::TerminalInputReader,
    Option<std::time::Instant>,
) {
    let controller = crate::repl::test_controller();
    let history = InteractiveHistory::with_file(10, temp.join("history.txt")).unwrap();
    let completions = CompletionSet::from_sources(Default::default());
    let batch = LiveBatch::new();
    let config = rho_harness_core::config::Config {
        config_dir: temp.to_path_buf(),
        sessions_dir: temp.join("sessions"),
        ..Default::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let mut auth = crate::auth::AuthStore::default();
    let _ = auth.set_key("anthropic", "test-key");
    let session = crate::repl::ReplSession::new(config, auth, None);
    let engine = session.new_engine(None).await.unwrap();
    let input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    let last_escape_time = None;
    (
        controller,
        history,
        completions,
        batch,
        session,
        engine,
        input,
        last_escape_time,
    )
}

#[test]
fn test_classify_event_resize_and_paste() {
    assert!(matches!(
        classify_event(Event::Resize(100, 40)),
        RawInput::Resize(100, 40)
    ));
    assert!(matches!(
        classify_event(Event::Paste("test".to_string())),
        RawInput::Paste(ref s) if s == "test"
    ));
    assert!(matches!(classify_event(Event::FocusGained), RawInput::Focus(true)));
    assert!(matches!(classify_event(Event::FocusLost), RawInput::Focus(false)));
}

#[test]
fn test_classify_event_keys_and_mouse() {
    let press_key = KeyEvent {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    assert!(matches!(classify_event(Event::Key(press_key)), RawInput::Key(_)));

    let release_key = KeyEvent {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
    };
    assert!(matches!(classify_event(Event::Key(release_key)), RawInput::Skip));

    let mouse_event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(matches!(classify_event(mouse_event), RawInput::Skip));
}

#[tokio::test]
async fn test_resize_and_focus_helpers() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, _, _, mut batch, session, _, _, _) = setup_idle_dispatch_harness(temp.path()).await;

    assert!(handle_resize(&mut controller, &session.renderer, &mut batch, 90, 35).is_ok());
    assert_eq!(controller.width(), 90);
    assert_eq!(controller.height(), 35);

    assert!(handle_focus(&mut controller, &session.renderer, &mut batch, true).is_ok());
    assert!(controller.focused());

    assert!(handle_focus(&mut controller, &session.renderer, &mut batch, false).is_ok());
    assert!(!controller.focused());
}

#[tokio::test]
async fn test_handle_raw_paste() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, _, completions, mut batch, session, _, _, _) = setup_idle_dispatch_harness(temp.path()).await;

    assert!(
        handle_raw_paste(
            &mut controller,
            &session.renderer,
            &mut batch,
            "hello world".to_string(),
            &completions
        )
        .is_ok()
    );
    assert_eq!(controller.state().editor().text(), "hello world");
}

#[tokio::test]
async fn test_process_raw_input_events() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, mut batch, mut session, mut engine, mut input, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;

    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    let resize_res = process_raw_input(
        &mut controller,
        Event::Resize(110, 45),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(resize_res, IdleInputResult::None));
    assert_eq!(controller.width(), 110);

    let focus_gain_res = process_raw_input(
        &mut controller,
        Event::FocusGained,
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(focus_gain_res, IdleInputResult::None));
    assert!(controller.focused());

    let focus_lost_res = process_raw_input(
        &mut controller,
        Event::FocusLost,
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(focus_lost_res, IdleInputResult::None));
    assert!(!controller.focused());

    let paste_res = process_raw_input(
        &mut controller,
        Event::Paste("line 1\nline 2".to_string()),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(paste_res, IdleInputResult::None));
    assert!(controller.state().editor().text().contains("line 1"));

    let skip_res = process_raw_input(
        &mut controller,
        Event::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        }),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(skip_res, IdleInputResult::None));
}

#[tokio::test]
async fn test_process_raw_input_typing_and_submit() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, mut batch, mut session, mut engine, mut input, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;

    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    let key_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
    let _ = process_raw_input(
        &mut controller,
        Event::Key(key_h),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    let key_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
    let _ = process_raw_input(
        &mut controller,
        Event::Key(key_i),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    assert_eq!(controller.state().editor().text(), "hi");

    let key_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = process_raw_input(
        &mut controller,
        Event::Key(key_enter),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    match res {
        IdleInputResult::Message(msg) => assert_eq!(msg.text, "hi"),
        _ => panic!("Expected message queued result"),
    }
}

async fn press(
    controller: &mut TerminalController<NullBackend>,
    resources: &mut EditorResources<'_>,
    ctx: &mut LiveIdleContext<'_, '_>,
    key: KeyEvent,
) -> IdleInputResult {
    let mut batch = LiveBatch::new();
    let mut input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    process_raw_input(controller, Event::Key(key), &mut batch, resources, &mut input, ctx)
        .await
        .unwrap()
}

#[tokio::test]
async fn prompts_submitted_from_the_editor_are_recorded_in_history() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, _, mut session, mut engine, _, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;
    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    for (text, modifiers) in [("run now", KeyModifiers::NONE), ("run later", KeyModifiers::ALT)] {
        controller.state_mut().editor_mut().set_text(text);
        let submitted = press(
            &mut controller,
            &mut resources,
            &mut ctx,
            KeyEvent::new(KeyCode::Enter, modifiers),
        )
        .await;
        assert!(matches!(submitted, IdleInputResult::Message(ref m) if m.text == text));
    }

    assert_eq!(resources.history.entries(), ["run now", "run later"]);
}

#[tokio::test]
async fn a_prompt_typed_to_a_shown_agent_is_recorded_as_typed() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, _, mut session, mut engine, _, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;
    let agent = engine.agent_monitor().start("subagents", "audit", "acme/smol", 10);
    agent.set_task("audit-1234");
    agent.finish(rho_harness_core::agents::AgentStatus::Done);
    crate::repl::live::agent_focus::show_agent(&mut controller, Some(engine.agent_monitor()), Some(agent.id()))
        .unwrap();
    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    controller.state_mut().editor_mut().set_text("also check tests");
    let submitted = press(
        &mut controller,
        &mut resources,
        &mut ctx,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .await;

    assert!(matches!(submitted, IdleInputResult::Message(ref m) if m.text.starts_with("Continue subagent task")));
    assert_eq!(resources.history.entries(), ["also check tests"]);
}

#[tokio::test]
async fn test_process_raw_input_shift_enter_inserts_newline() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, mut batch, mut session, mut engine, mut input, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;

    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    let key_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    let _ = process_raw_input(
        &mut controller,
        Event::Key(key_a),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    let key_shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    let res = process_raw_input(
        &mut controller,
        Event::Key(key_shift_enter),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    assert!(matches!(res, IdleInputResult::None));
    assert_eq!(controller.state().editor().text(), "a\n");

    let key_b = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE);
    let _ = process_raw_input(
        &mut controller,
        Event::Key(key_b),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    assert_eq!(controller.state().editor().text(), "a\nb");

    let key_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = process_raw_input(
        &mut controller,
        Event::Key(key_enter),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    match res {
        IdleInputResult::Message(msg) => assert_eq!(msg.text, "a\nb"),
        _ => panic!("Expected message queued result"),
    }
}

#[tokio::test]
async fn test_process_raw_input_ctrl_d_exit() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, mut batch, mut session, mut engine, mut input, mut last_escape_time) =
        setup_idle_dispatch_harness(temp.path()).await;

    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };
    let mut ctx = LiveIdleContext {
        session: &mut session,
        engine: &mut engine,
        last_escape_time: &mut last_escape_time,
    };

    let key_ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    let res = process_raw_input(
        &mut controller,
        Event::Key(key_ctrl_d),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();

    assert!(matches!(res, IdleInputResult::Exit));

    controller.state_mut().editor_mut().set_text("not empty");
    let res_non_empty = process_raw_input(
        &mut controller,
        Event::Key(key_ctrl_d),
        &mut batch,
        &mut resources,
        &mut input,
        &mut ctx,
    )
    .await
    .unwrap();
    assert!(matches!(res_non_empty, IdleInputResult::None));
}

#[tokio::test]
async fn test_misc_action_completion_and_dequeue() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, completions, mut batch, _, _, mut input, _) =
        setup_idle_dispatch_harness(temp.path()).await;

    let mut resources = EditorResources {
        history: &mut history,
        completions: &completions,
    };

    assert!(
        handle_misc_action(
            &mut controller,
            &InputAction::Complete,
            &mut batch,
            &mut resources,
            &mut input
        )
        .await
        .is_ok()
    );

    assert!(
        handle_misc_action(
            &mut controller,
            &InputAction::DequeueQueued,
            &mut batch,
            &mut resources,
            &mut input
        )
        .await
        .is_ok()
    );

    controller.state_mut().push_front_queued(QueuedMessage {
        text: "queued item 2".to_string(),
        kind: QueueKind::FollowUp,
    });
    controller.state_mut().push_front_queued(QueuedMessage {
        text: "queued item 1".to_string(),
        kind: QueueKind::Steering,
    });

    assert!(
        handle_misc_action(
            &mut controller,
            &InputAction::DequeueQueued,
            &mut batch,
            &mut resources,
            &mut input
        )
        .await
        .is_ok()
    );

    assert_eq!(controller.state().editor().text(), "queued item 1\nqueued item 2");
}
