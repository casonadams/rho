use super::common::HistoryTerminal;
use crate::repl::interactive::InteractiveHistory;
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::idle::modal_action::{ModalActionContext, apply_modal_key_result};
use crate::repl::live::modal::ModalKeyResult;
use crate::ui::interactive::{InteractiveState, TerminalController};

async fn setup_modal_action_harness(
    temp: &std::path::Path,
) -> (
    TerminalController<HistoryTerminal>,
    InteractiveHistory,
    LiveBatch,
    crate::repl::ReplSession,
    crate::engine::AgentEngine,
    crate::repl::input_reader::TerminalInputReader,
) {
    let controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let history = InteractiveHistory::with_file(10, temp.join("history.txt")).unwrap();
    let batch = LiveBatch::new();
    let config = rho_harness_core::config::Config {
        config_dir: temp.to_path_buf(),
        sessions_dir: temp.join("sessions"),
        ..Default::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let mut auth = crate::auth::AuthStore::default();
    let _ = auth.set_api_key("anthropic", "test-key");
    let session = crate::repl::ReplSession::new(config.clone(), auth.clone(), None);
    let engine = crate::platform::agent_engine(config, auth, None).await.unwrap();
    let input = crate::repl::input_reader::TerminalInputReader::spawn_dummy();
    (controller, history, batch, session, engine, input)
}

#[tokio::test]
async fn test_modal_action_handled_and_not_handled() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(ModalKeyResult::Handled, ctx, &mut batch)
            .await
            .unwrap()
    );

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        !apply_modal_key_result(ModalKeyResult::NotHandled, ctx, &mut batch)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_modal_action_menu_openers() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(
            ModalKeyResult::OpenModelSelector { save_as_default: false },
            ctx,
            &mut batch
        )
        .await
        .unwrap()
    );
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");
    controller.state_mut().pop_modal();

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(ModalKeyResult::OpenToolsMenu, ctx, &mut batch)
            .await
            .unwrap()
    );
    assert_eq!(controller.state().active_modal().unwrap().title, "Tools & Permissions");
    controller.state_mut().pop_modal();

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(ModalKeyResult::OpenSearchEngineSelector, ctx, &mut batch)
            .await
            .unwrap()
    );
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Search Engine");
    controller.state_mut().pop_modal();

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(ModalKeyResult::OpenGuardModelSelector, ctx, &mut batch)
            .await
            .unwrap()
    );
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Guard Model");
}

#[tokio::test]
async fn test_modal_action_guard_model_selected() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(
            ModalKeyResult::GuardModelSelected {
                model: "qwen2.5-coder:7b".to_string(),
                provider: "local".to_string(),
            },
            ctx,
            &mut batch
        )
        .await
        .unwrap()
    );
    assert_eq!(session.config.guard_model(), Some("local/qwen2.5-coder:7b"));
    assert_eq!(engine.config.guard_model(), Some("local/qwen2.5-coder:7b"));

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(
            ModalKeyResult::GuardModelSelected {
                model: "none".to_string(),
                provider: "none".to_string(),
            },
            ctx,
            &mut batch
        )
        .await
        .unwrap()
    );
    assert_eq!(session.config.guard_model(), None);
    assert_eq!(engine.config.guard_model(), None);
}

#[tokio::test]
async fn test_modal_action_search_engine_selected() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(
            ModalKeyResult::SearchEngineSelected {
                engine: "brave".to_string()
            },
            ctx,
            &mut batch
        )
        .await
        .unwrap()
    );
    assert_eq!(session.config.tools.web.search.default, "brave");
    assert_eq!(engine.config.tools.web.search.default, "brave");
}

#[tokio::test]
async fn test_modal_action_help_commands() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let commands = ["/settings", "/model", "/resume", "/tree", "/mcp", "/login", "/unknown"];
    for cmd in commands {
        let ctx = ModalActionContext {
            controller: &mut controller,
            history: &mut history,
            session: &mut session,
            engine: &mut engine,
            input: &mut input,
        };
        assert!(
            apply_modal_key_result(
                ModalKeyResult::HelpCommandSelected {
                    command: cmd.to_string()
                },
                ctx,
                &mut batch
            )
            .await
            .unwrap()
        );
        controller.state_mut().pop_modal();
    }
}

#[tokio::test]
async fn test_modal_action_tool_settings_toggled() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let actions = [
        ModalKeyResult::WebSearchToggled { enabled: true },
        ModalKeyResult::WebSearchToggled { enabled: false },
        ModalKeyResult::WebFetchToggled { enabled: true },
        ModalKeyResult::WebFetchToggled { enabled: false },
        ModalKeyResult::McpToggled { enabled: true },
        ModalKeyResult::McpToggled { enabled: false },
        ModalKeyResult::PermissionToggled { enabled: true },
        ModalKeyResult::PermissionToggled { enabled: false },
    ];

    for action in actions {
        let ctx = ModalActionContext {
            controller: &mut controller,
            history: &mut history,
            session: &mut session,
            engine: &mut engine,
            input: &mut input,
        };
        assert!(apply_modal_key_result(action, ctx, &mut batch).await.unwrap());
    }

    assert!(!session.config.tools.web.search.enabled);
    assert!(!session.config.tools.web.fetch.enabled);
    assert!(!session.config.mcp.enabled);
    assert!(!session.config.permission.enabled);
}

#[tokio::test]
async fn test_modal_action_ui_settings_toggled() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    session.config.mcp.servers.insert(
        "test_srv".to_string(),
        rho_harness_core::config::McpServerConfig::stdio("test", vec![]),
    );

    let actions = [
        ModalKeyResult::McpServerToggled {
            server: "test_srv".to_string(),
        },
        ModalKeyResult::McpServerToggled {
            server: "nonexistent".to_string(),
        },
        ModalKeyResult::SemanticSearchToggled { enabled: true },
        ModalKeyResult::SemanticSearchToggled { enabled: false },
    ];

    for action in actions {
        let ctx = ModalActionContext {
            controller: &mut controller,
            history: &mut history,
            session: &mut session,
            engine: &mut engine,
            input: &mut input,
        };
        assert!(apply_modal_key_result(action, ctx, &mut batch).await.unwrap());
    }

    assert!(!session.config.semantic_search);
    assert!(!engine.config.semantic_search);
    assert!(!session.config.mcp.servers["test_srv"].enabled);
}

#[tokio::test]
async fn test_modal_action_session_operations() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let node_action = ModalKeyResult::TreeNodeSelected {
        node_id: "nonexistent-node".to_string(),
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(node_action, ctx, &mut batch).await.unwrap());

    let label_action = ModalKeyResult::NodeLabelUpdated {
        node_id: "nonexistent-node".to_string(),
        label: "checkpoint-a".to_string(),
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(label_action, ctx, &mut batch).await.unwrap());

    let unlabel_action = ModalKeyResult::NodeLabelUpdated {
        node_id: "nonexistent-node".to_string(),
        label: "".to_string(),
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(unlabel_action, ctx, &mut batch).await.unwrap());

    let delete_action = ModalKeyResult::SessionDeleted {
        session_id: "nonexistent-session".to_string(),
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(delete_action, ctx, &mut batch).await.unwrap());

    let resume_action = ModalKeyResult::SessionSelected {
        session_id: engine.session_manager.session_id.clone(),
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(resume_action, ctx, &mut batch).await.unwrap());
}

#[tokio::test]
async fn test_modal_action_selection_operations() {
    let temp = tempfile::tempdir().unwrap();
    let (mut controller, mut history, mut batch, mut session, mut engine, mut input) =
        setup_modal_action_harness(temp.path()).await;

    let model_action = ModalKeyResult::ModelSelected {
        model: "claude-3-5-sonnet-20241022".to_string(),
        provider: "anthropic".to_string(),
        save_as_default: false,
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(model_action, ctx, &mut batch).await.unwrap());
    assert_eq!(session.config.model, "claude-3-5-sonnet-20241022");

    let model_default_action = ModalKeyResult::ModelSelected {
        model: "claude-3-5-haiku-20241022".to_string(),
        provider: "anthropic".to_string(),
        save_as_default: true,
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(model_default_action, ctx, &mut batch)
            .await
            .unwrap()
    );
    assert_eq!(session.config.model, "claude-3-5-haiku-20241022");

    let thinking_action = ModalKeyResult::ThinkingLevelSelected {
        level: Some("high".to_string()),
        save_as_default: false,
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(apply_modal_key_result(thinking_action, ctx, &mut batch).await.unwrap());
    assert_eq!(session.config.thinking_level.as_deref(), Some("high"));

    let thinking_default_action = ModalKeyResult::ThinkingLevelSelected {
        level: None,
        save_as_default: true,
    };
    let ctx = ModalActionContext {
        controller: &mut controller,
        history: &mut history,
        session: &mut session,
        engine: &mut engine,
        input: &mut input,
    };
    assert!(
        apply_modal_key_result(thinking_default_action, ctx, &mut batch)
            .await
            .unwrap()
    );
    assert_eq!(session.config.thinking_level, None);
}
