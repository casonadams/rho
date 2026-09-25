use super::shell::{ShellAction, handle_shell_command, submitted_input_rows};
use crate::ui::TerminalRenderer;

#[test]
fn submitted_input_rows_calculates_wrapped_height() {
    assert_eq!(submitted_input_rows("hello", 80), 1);
    assert_eq!(submitted_input_rows(&"x".repeat(78), 80), 2);
    assert_eq!(submitted_input_rows("one\ntwo", 80), 2);
    assert_eq!(submitted_input_rows("界界", 5), 2);
}

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
    handle_command_result, handle_config_auth_result, handle_model_change, handle_session_branching_result,
    handle_session_manage_result, handle_terminal_result, has_assistant_turn, name_session, resume_session,
    rewind_session, should_summarize_branch, show_session_summaries, show_tree,
};
use crate::repl::commands::CommandResult;
use rho_harness_core::session::tree::{TreeNodeData, TreeNodeKind};

async fn setup_test_session(dir: &std::path::Path) -> (crate::repl::ReplSession, crate::engine::AgentEngine) {
    let config = rho_harness_core::config::Config {
        config_dir: dir.to_path_buf(),
        sessions_dir: dir.join("sessions"),
        ..Default::default()
    };
    let auth = rho_engine::auth::AuthStore::default();
    let session = crate::repl::ReplSession::new(config.clone(), auth.clone(), None);
    let engine = crate::platform::agent_engine(config, auth, None).await.unwrap();
    (session, engine)
}

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
    let temp = tempfile::tempdir().unwrap();
    let (session, engine) = setup_test_session(temp.path()).await;

    assert!(show_tree(&session, &engine).await.is_ok());
    assert!(show_session_summaries(&session).is_ok());
}

#[tokio::test]
async fn test_session_management_helpers() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    let sid = engine.session_manager.session_id.clone();
    assert!(name_session("custom-session", &session, &engine).await.is_ok());
    assert_eq!(
        engine.session_manager.cached_session_name().as_deref(),
        Some("custom-session")
    );

    rewind_session(0, &session, &engine).await;
    rewind_session(999, &session, &engine).await;

    assert!(resume_session(&sid, &mut session, &mut engine).await.is_ok());
    assert_eq!(session.resume_id.as_deref(), Some(sid.as_str()));
}

#[tokio::test]
async fn test_fork_and_clone_session() {
    let temp = tempfile::tempdir().unwrap();
    let (session, engine) = setup_test_session(temp.path()).await;

    assert!(fork_session(&session, &engine, None).await.is_ok());
    assert!(clone_session(&session, &engine).await.is_ok());
}

#[tokio::test]
async fn test_handle_session_manage_result_arms() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;
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
async fn test_handle_session_branching_result_arms() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    let res = handle_session_branching_result(&CommandResult::Tree, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(&CommandResult::OpenTreeSelector, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(true)));

    let res = handle_session_branching_result(
        &CommandResult::ForkSession { turn_or_node_id: None },
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
async fn test_handle_auth_and_config_results() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    let orig_provider = session.config.provider.clone();
    handle_model_change("new-model", None, &mut session, &mut engine).await;
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
            new_model: "another-model".into(),
            new_provider: None,
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
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

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

    assert!(handle_command_group(&CommandResult::Tree, &mut session, &mut engine).await);
    assert!(!handle_command_group(&CommandResult::Exit, &mut session, &mut engine).await);

    let outcome = handle_command_result(CommandResult::Tree, &mut session, &mut engine).await;
    assert!(matches!(outcome, Ok(DispatchOutcome::Continue)));

    let outcome = handle_command_result(CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(outcome, Ok(DispatchOutcome::Break)));
}

#[tokio::test]
async fn test_maybe_summarize_abandoned_and_branch_switch() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    assert_eq!(
        super::dispatch::maybe_summarize_abandoned(&[], &engine, false).await,
        None
    );

    let assistant_node = TreeNodeData {
        id: "a1".into(),
        parent_id: None,
        timestamp: chrono::Utc::now(),
        kind: TreeNodeKind::AssistantTurn,
        messages: vec![],
        label: None,
        metadata: None,
    };
    assert_eq!(
        super::dispatch::maybe_summarize_abandoned(&[&assistant_node], &engine, false).await,
        None
    );

    let active_leaf = engine
        .session_manager
        .active_leaf_id()
        .await
        .unwrap()
        .unwrap_or_default();
    assert!(
        super::dispatch::switch_active_branch(active_leaf, &mut session, &mut engine)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn test_try_login_action_variants() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    let res = super::dispatch::try_login_action(&CommandResult::OpenLoginSelector, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(Some(_))));

    let res = super::dispatch::try_login_action(
        &CommandResult::Login {
            provider: Some("local".to_string()),
        },
        &mut session,
        &mut engine,
    )
    .await;
    assert!(matches!(res, Ok(Some(_))));

    let res = super::dispatch::try_login_action(&CommandResult::Exit, &mut session, &mut engine).await;
    assert!(matches!(res, Ok(None)));
}

#[test]
fn test_build_emacs_edit_mode_newline_bindings() {
    use reedline::{EditCommand, KeyCode, KeyModifiers, ReedlineEvent};

    let keybindings = super::build_emacs_keybindings();
    for modifier in [KeyModifiers::ALT, KeyModifiers::SHIFT, KeyModifiers::CONTROL] {
        let binding = keybindings.find_binding(modifier, KeyCode::Enter);
        assert_eq!(binding, Some(ReedlineEvent::Edit(vec![EditCommand::InsertNewline])));
    }
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

    assert_eq!(super::handle_line_control_signal(&Signal::CtrlC, &renderer), Some(true));
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
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

    let err_sig = Err(std::io::Error::other("read error"));
    assert!(
        !super::handle_line_signal(err_sig, &mut session, &mut engine, false)
            .await
            .unwrap()
    );

    assert!(
        super::handle_line_signal(Ok(Signal::CtrlC), &mut session, &mut engine, false)
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
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut engine) = setup_test_session(temp.path()).await;

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
    let temp = tempfile::tempdir().unwrap();
    let (session, engine) = setup_test_session(temp.path()).await;

    let mut is_first = true;
    super::step_line_prompt(&session, &engine, &mut is_first);
    assert!(!is_first);

    super::step_line_prompt(&session, &engine, &mut is_first);
    assert!(!is_first);
}
