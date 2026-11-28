use super::shell::{ShellAction, handle_shell_command};
use crate::ui::TerminalRenderer;
use test_case::test_case;

#[tokio::test]
async fn plain_input_is_passthrough() {
    let renderer = TerminalRenderer::default();
    let action = handle_shell_command("plain prompt", &renderer).await;
    match action {
        ShellAction::Passthrough => {}
        _ => panic!("Expected ShellAction::Passthrough"),
    }
}

use super::dispatch::{
    DispatchOutcome, clone_session, compact_context, fork_session, handle_auth_actions, handle_command_group,
    handle_command_result, handle_config_auth_result, handle_session_branching_result, handle_session_manage_result,
    handle_terminal_result, show_session_summaries, show_tree,
};
use crate::repl::branch_summary::{has_assistant_turn, should_summarize_branch};
use crate::repl::commands::CommandResult;
use crate::repl::model_switch::switch_session_model;
use crate::repl::test_session_and_engine;
use rho_harness_core::provider::ModelRoute;
use rho_harness_core::session::tree::{TreeNodeData, TreeNodeKind};

#[test]
fn test_has_assistant_turn_detection() {
    assert!(!has_assistant_turn(&[]));

    let user_node = TreeNodeData {
        id: "u1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: vec![],
        label: None,
        metadata: None,
    };
    assert!(!has_assistant_turn(&[&user_node]));

    let assistant_node = TreeNodeData {
        id: "a1".into(),
        parent_id: Some("u1".into()),
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![],
        label: None,
        metadata: None,
    };
    assert!(has_assistant_turn(&[&user_node, &assistant_node]));
}

#[test]
fn test_should_summarize_branch_inputs() {
    assert!(should_summarize_branch(""));
    assert!(should_summarize_branch("   "));
    assert!(should_summarize_branch("y"));
    assert!(should_summarize_branch("Y"));
    assert!(should_summarize_branch("yes"));
    assert!(should_summarize_branch("YES"));
    assert!(!should_summarize_branch("n"));
    assert!(!should_summarize_branch("no"));
    assert!(!should_summarize_branch("other"));
}

#[tokio::test]
async fn test_show_tree_and_session_summaries() {
    let temp = rho_test_support::TestDir::new();
    let (session, engine) = test_session_and_engine(&temp).await;

    assert!(show_tree(&session, &engine).await.is_ok());
    assert!(show_session_summaries(&session).is_ok());
}

#[tokio::test]
async fn test_session_management_helpers() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    let sid = engine.session_manager.session_id.clone();
    let name = CommandResult::NameSession {
        name: "custom-session".into(),
    };
    assert!(
        handle_session_manage_result(&name, &mut session, &mut engine)
            .await
            .is_ok()
    );
    assert_eq!(
        engine.session_manager.cached_session_name().as_deref(),
        Some("custom-session")
    );

    for turn in [0, 999] {
        let rewind = CommandResult::Rewind { turn };
        assert!(
            handle_session_manage_result(&rewind, &mut session, &mut engine)
                .await
                .is_ok()
        );
    }

    let resume = CommandResult::ResumeSession {
        session_id: sid.clone(),
    };
    assert!(
        handle_session_manage_result(&resume, &mut session, &mut engine)
            .await
            .is_ok()
    );
    assert_eq!(session.resume_id.as_deref(), Some(sid.as_str()));
}

#[tokio::test]
async fn test_fork_and_clone_session() {
    let temp = rho_test_support::TestDir::new();
    let (session, engine) = test_session_and_engine(&temp).await;

    assert!(fork_session(&session, &engine, None, None).await.is_ok());
    assert!(clone_session(&session, &engine).await.is_ok());
}

#[tokio::test]
async fn test_handle_session_manage_result_arms() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let sid = engine.session_manager.session_id.clone();

    let res = handle_session_manage_result(
        &CommandResult::ResumeSession { session_id: sid },
        &mut session,
        &mut engine,
    )
    .await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_manage_result(&CommandResult::OpenSessionSelector, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_manage_result(
        &CommandResult::NameSession {
            name: "test-sess".into(),
        },
        &mut session,
        &mut engine,
    )
    .await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_manage_result(&CommandResult::Rewind { turn: 0 }, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_manage_result(&CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(false)));
}

#[tokio::test]
async fn name_session_reports_the_quoted_name_in_line_mode() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;

    let res = handle_session_manage_result(
        &CommandResult::NameSession { name: "custom".into() },
        &mut session,
        &mut engine,
    )
    .await;

    assert!(matches!(res, Ok(true)));
    assert!(crate::repl::collected_output(&mut events).contains("Session name: \"custom\""));
}

#[tokio::test]
async fn rewind_reports_the_turn_and_retained_count_in_line_mode() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;

    let res = handle_session_manage_result(&CommandResult::Rewind { turn: 999 }, &mut session, &mut engine).await;

    assert!(matches!(res, Ok(true)));
    assert!(crate::repl::collected_output(&mut events).contains("Rewound context to Turn 999 (0 messages retained)"));
}

#[tokio::test]
async fn resume_reports_a_missing_workspace_in_line_mode() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let sid = engine.session_manager.session_id.clone();
    let scratch = rho_test_support::TestDir::new();
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    rho_harness_core::session::SessionManager::record_session_for_cwd(&session.config.sessions_dir, &workspace, &sid)
        .unwrap();
    std::fs::remove_dir_all(&workspace).unwrap();
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;

    let res = handle_session_manage_result(
        &CommandResult::ResumeSession { session_id: sid },
        &mut session,
        &mut engine,
    )
    .await;

    assert!(matches!(res, Ok(true)));
    assert!(crate::repl::collected_output(&mut events).contains("no longer exists"));
}

#[tokio::test]
async fn test_handle_session_branching_result_arms() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    let res = handle_session_branching_result(&CommandResult::Tree, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(&CommandResult::OpenTreeSelector, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(
        &CommandResult::ForkSession {
            target: None,
            turn_or_node_id: None,
        },
        &mut session,
        &mut engine,
    )
    .await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(&CommandResult::CloneSession, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(&CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(false)));
}

#[tokio::test]
async fn a_failed_model_switch_changes_neither_the_session_nor_the_engine() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let before = (session.config.model.clone(), session.config.provider.clone());
    assert_eq!(before, (engine.config.model.clone(), engine.config.provider.clone()));

    let switched = switch_session_model(
        &ModelRoute::new("no-such-provider", "some-model"),
        &mut session,
        &mut engine,
    )
    .await;

    assert!(!switched);
    assert_eq!((session.config.model.clone(), session.config.provider.clone()), before);
    assert_eq!((engine.config.model.clone(), engine.config.provider.clone()), before);
}

#[tokio::test]
async fn a_successful_model_switch_updates_the_session_and_the_engine_together() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    let route = ModelRoute::new("ollama", "new-model");

    assert!(switch_session_model(&route, &mut session, &mut engine).await);

    assert_eq!(session.config.route(), route);
    assert_eq!(engine.config.model, "new-model");
    assert_eq!(engine.config.provider, session.config.provider);
}

#[tokio::test]
async fn test_handle_auth_and_config_results() {
    let temp = rho_test_support::TestDir::new();
    let _home = rho_test_support::EnvVarGuard::set("RHO_HOME", temp.to_str().unwrap());
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    let orig_provider = session.config.provider.clone();
    assert!(switch_session_model(&ModelRoute::new(&orig_provider, "new-model"), &mut session, &mut engine).await);
    assert_eq!(session.config.model, "new-model");
    assert_eq!(session.config.provider, orig_provider);
    let res = handle_auth_actions(&CommandResult::Logout { provider: None }, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_auth_actions(&CommandResult::Reload, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_config_auth_result(&CommandResult::ClearContext, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_config_auth_result(
        &CommandResult::ModelChanged {
            route: ModelRoute::new(&orig_provider, "another-model"),
        },
        &mut session,
        &mut engine,
    )
    .await;
    assert!(matches!(res, Ok(true)));

    let res = handle_config_auth_result(&CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(false)));
}

#[tokio::test]
async fn test_terminal_and_command_result_dispatch() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    compact_context(&session, &engine, None).await;

    let term_res = handle_terminal_result(CommandResult::Exit, &session, &engine).await;
    assert!(matches!(term_res, Ok(DispatchOutcome::Break)));

    let term_res = handle_terminal_result(CommandResult::Compact { instructions: None }, &session, &engine).await;
    assert!(matches!(term_res, Ok(DispatchOutcome::Continue)));

    let term_res = handle_terminal_result(
        CommandResult::ExpandedPrompt {
            text: "expanded test".into(),
        },
        &session,
        &engine,
    )
    .await;
    assert!(matches!(term_res, Ok(DispatchOutcome::RunTurn(text)) if text == "expanded test"));

    assert!(
        handle_command_group(&CommandResult::Tree, &mut session, &mut engine)
            .await
            .unwrap()
    );
    assert!(
        !handle_command_group(&CommandResult::Exit, &mut session, &mut engine)
            .await
            .unwrap()
    );

    let outcome = handle_command_result(CommandResult::Tree, &mut session, &mut engine).await;
    assert!(matches!(outcome, Ok(DispatchOutcome::Continue)));

    let outcome = handle_command_result(CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(outcome, Ok(DispatchOutcome::Break)));
}

#[tokio::test]
async fn test_maybe_summarize_abandoned_and_branch_switch() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    engine
        .session_manager
        .append_messages(
            &engine.session_manager.session_id,
            vec![
                rho_harness_core::model::ChatMessage::user("hello"),
                rho_harness_core::model::ChatMessage::assistant("hi"),
            ],
        )
        .await
        .unwrap();
    let active_leaf = engine
        .session_manager
        .active_leaf_id()
        .await
        .unwrap()
        .expect("a stored turn creates a leaf");
    assert!(
        super::dispatch::switch_active_branch(active_leaf, &mut session, &mut engine)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn abandoned_branch_without_assistant_turn_is_not_offered_a_summary() {
    let temp = rho_test_support::TestDir::new();
    let (_session, engine) = test_session_and_engine(&temp).await;
    let user_node = TreeNodeData::turn(TreeNodeKind::UserTurn, None, chrono::Utc::now(), vec![]);
    let mut reader = std::io::Cursor::new(b"y\n".to_vec());
    let mut writer = Vec::new();

    let summary = super::dispatch::maybe_summarize_abandoned(
        &[&user_node],
        &engine,
        &mut reader,
        &mut writer,
        &TerminalRenderer::default(),
    )
    .await;

    assert_eq!(summary, None);
    assert!(writer.is_empty(), "no prompt is printed");
}

#[test_case(b"n\n", false; "n")]
#[test_case(b"no\n", false; "no")]
#[test_case(b"y\n", true; "y")]
#[test_case(b"yes\n", true; "yes")]
#[test_case(b"\n", true; "empty answer")]
#[tokio::test]
async fn abandoned_branch_summary_follows_the_typed_answer(answer: &[u8], expect_summary: bool) {
    let temp = rho_test_support::TestDir::new();
    let (_session, engine) = test_session_and_engine(&temp).await;
    let assistant_node = TreeNodeData::turn(
        TreeNodeKind::AssistantTurn,
        None,
        chrono::Utc::now(),
        vec![
            rho_harness_core::model::ChatMessage::user("hello"),
            rho_harness_core::model::ChatMessage::assistant("hi"),
        ],
    );
    let mut reader = std::io::Cursor::new(answer.to_vec());
    let mut writer = Vec::new();

    let summary = super::dispatch::maybe_summarize_abandoned(
        &[&assistant_node],
        &engine,
        &mut reader,
        &mut writer,
        &TerminalRenderer::default(),
    )
    .await;

    assert_eq!(summary.is_some(), expect_summary);
    assert_eq!(
        String::from_utf8(writer).unwrap(),
        crate::repl::branch_summary::SUMMARY_PROMPT
    );
}

struct FailingReader;

impl std::io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("stdin unavailable"))
    }
}

impl std::io::BufRead for FailingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        Err(std::io::Error::other("stdin unavailable"))
    }

    fn consume(&mut self, _amt: usize) {}
}

#[tokio::test]
async fn a_summary_prompt_read_error_is_reported_and_skips_the_summary() {
    let temp = rho_test_support::TestDir::new();
    let (_session, engine) = test_session_and_engine(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    let assistant_node = TreeNodeData::turn(
        TreeNodeKind::AssistantTurn,
        None,
        chrono::Utc::now(),
        vec![rho_harness_core::model::ChatMessage::assistant("hi")],
    );
    let mut writer = Vec::new();

    let summary = super::dispatch::maybe_summarize_abandoned(
        &[&assistant_node],
        &engine,
        &mut FailingReader,
        &mut writer,
        &renderer,
    )
    .await;

    assert_eq!(summary, None);
    let printed = crate::repl::collected_output(&mut events);
    assert!(printed.contains("stdin unavailable"), "{printed}");
}

#[tokio::test]
async fn a_failed_command_reports_its_error_and_keeps_the_session_going() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;
    let (renderer, mut events) = crate::repl::collecting_renderer();
    session.renderer = renderer;

    let logout = CommandResult::Logout {
        provider: Some("claude".to_string()),
    };
    let outcome = handle_command_result(logout, &mut session, &mut engine).await;

    assert!(matches!(outcome, Ok(DispatchOutcome::Continue)));
    let printed = crate::repl::collected_output(&mut events);
    assert!(printed.contains("No stored credentials for claude"), "{printed}");
}

#[test_case(reedline::KeyModifiers::ALT; "alt")]
#[test_case(reedline::KeyModifiers::SHIFT; "shift")]
#[test_case(reedline::KeyModifiers::CONTROL; "control")]
fn test_build_emacs_edit_mode_newline_bindings(modifier: reedline::KeyModifiers) {
    use reedline::{EditCommand, KeyCode, ReedlineEvent};

    let keybindings = super::build_emacs_keybindings();
    let binding = keybindings.find_binding(modifier, KeyCode::Enter);
    assert_eq!(binding, Some(ReedlineEvent::Edit(vec![EditCommand::InsertNewline])));
}

#[tokio::test]
async fn test_resolve_user_prompt() {
    let renderer = TerminalRenderer::default();
    assert_eq!(
        super::resolve_user_prompt("plain prompt", &renderer).await,
        Some("plain prompt".to_string())
    );
}

#[test]
fn test_handle_line_control_signal() {
    use reedline::Signal;
    let renderer = TerminalRenderer::default();

    assert_eq!(
        super::handle_line_control_signal(&Signal::CtrlC(String::new()), &renderer),
        Some(true)
    );
    assert_eq!(
        super::handle_line_control_signal(&Signal::CtrlD, &renderer),
        Some(false)
    );
    assert_eq!(
        super::handle_line_control_signal(&Signal::Success("hello".to_string()), &renderer),
        None
    );
}

#[tokio::test]
async fn test_handle_line_signal_control_paths() {
    use reedline::Signal;
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    let err_sig = Err(std::io::Error::other("read error"));
    assert!(
        !super::handle_line_signal(err_sig, &mut session, &mut engine, false)
            .await
            .unwrap()
    );

    assert!(
        super::handle_line_signal(Ok(Signal::CtrlC(String::new())), &mut session, &mut engine, false)
            .await
            .unwrap()
    );
    assert!(
        !super::handle_line_signal(Ok(Signal::CtrlD), &mut session, &mut engine, false)
            .await
            .unwrap()
    );

    let empty_sig = Ok(Signal::Success("   \n".to_string()));
    assert!(
        super::handle_line_signal(empty_sig, &mut session, &mut engine, false)
            .await
            .unwrap()
    );

    let exit_sig = Ok(Signal::Success("/exit".to_string()));
    assert!(
        !super::handle_line_signal(exit_sig, &mut session, &mut engine, false)
            .await
            .unwrap()
    );

    let help_sig = Ok(Signal::Success("/help".to_string()));
    assert!(
        super::handle_line_signal(help_sig, &mut session, &mut engine, false)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_apply_dispatch_outcome() {
    let temp = rho_test_support::TestDir::new();
    let (mut session, mut engine) = test_session_and_engine(&temp).await;

    assert!(
        super::apply_dispatch_outcome(DispatchOutcome::Continue, &mut session, &mut engine)
            .await
            .unwrap()
    );
    assert!(
        !super::apply_dispatch_outcome(DispatchOutcome::Break, &mut session, &mut engine)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_step_line_prompt() {
    let temp = rho_test_support::TestDir::new();
    let (session, engine) = test_session_and_engine(&temp).await;

    let mut is_first = true;
    super::step_line_prompt(&session, &engine, &mut is_first);
    assert!(!is_first);

    super::step_line_prompt(&session, &engine, &mut is_first);
    assert!(!is_first);
}

struct ScriptedLines(std::collections::VecDeque<std::io::Result<reedline::Signal>>);

impl super::LineSource for ScriptedLines {
    async fn read_next(mut self) -> crate::error::Result<(Self, std::io::Result<reedline::Signal>)> {
        let sig = self.0.pop_front().expect("line loop read past its script");
        Ok((self, sig))
    }
}

#[tokio::test]
async fn run_line_event_loop_stops_on_exit_or_input_error() {
    use reedline::Signal;
    let scripts: [Vec<std::io::Result<Signal>>; 2] = [
        vec![
            Ok(Signal::Success("   ".to_string())),
            Ok(Signal::CtrlC(String::new())),
            Ok(Signal::Success("/help".to_string())),
            Ok(Signal::Success("/exit".to_string())),
        ],
        vec![
            Ok(Signal::Success("/help".to_string())),
            Err(std::io::Error::other("read error")),
        ],
    ];
    let temp = rho_test_support::TestDir::new();
    for script in scripts {
        let (mut session, engine) = test_session_and_engine(&temp).await;
        super::run_line_event_loop(ScriptedLines(script.into()), &mut session, engine, false)
            .await
            .unwrap();
    }
}
