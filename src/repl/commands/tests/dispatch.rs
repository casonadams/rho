use super::{collected_output, collecting_renderer, test_context};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use rho_engine::auth::AuthStore;

#[tokio::test]
async fn help_is_emitted_through_the_renderer() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/help", &mut context).await.unwrap();

    assert!(matches!(result, Some(CommandResult::Continue)));
    let output = collected_output(&mut events);
    assert!(output.contains("/model [model] [provider]"));
    assert!(output.contains("Current session"));
}

#[tokio::test]
async fn login_is_dispatched_without_collecting_credentials() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/login chatgpt", &mut context)
        .await
        .unwrap();
    assert!(matches!(
        result,
        Some(CommandResult::Login {
            provider: Some(provider)
        }) if provider == "chatgpt"
    ));
    let result_ag = SlashCommandHandler::handle("/login antigravity", &mut context)
        .await
        .unwrap();
    assert!(matches!(
        result_ag,
        Some(CommandResult::Login {
            provider: Some(provider)
        }) if provider == "antigravity"
    ));
}

#[tokio::test]
async fn model_switch_is_emitted_and_updates_configuration() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/model gpt-4o openai", &mut context)
        .await
        .unwrap();

    assert!(matches!(result, Some(CommandResult::ModelChanged { .. })));
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.provider, "openai");
    assert!(collected_output(&mut events).contains("Model: gpt-4o (openai)"));
}

#[tokio::test]
async fn compact_tree_and_rewind_commands_return_expected_results() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let compact = SlashCommandHandler::handle("/compact keep tests only", &mut context)
        .await
        .unwrap();
    assert_eq!(
        compact,
        Some(CommandResult::Compact {
            instructions: Some("keep tests only".to_string())
        })
    );

    let tree = SlashCommandHandler::handle("/tree", &mut context).await.unwrap();
    assert_eq!(tree, Some(CommandResult::OpenTreeSelector));

    let rewind = SlashCommandHandler::handle("/rewind", &mut context).await.unwrap();
    assert_eq!(rewind, Some(CommandResult::Continue));
}

#[tokio::test]
async fn reload_command_requests_engine_reload() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/reload", &mut context).await.unwrap();
    assert_eq!(result, Some(CommandResult::Reload));

    let with_args = SlashCommandHandler::handle("/reload now", &mut context).await.unwrap();
    assert_eq!(with_args, Some(CommandResult::Reload));
}

#[tokio::test]
async fn test_new_and_thinking_commands() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let new_res = SlashCommandHandler::handle("/new", &mut context).await.unwrap();
    assert_eq!(new_res, Some(CommandResult::ClearContext));

    let think_res = SlashCommandHandler::handle("/thinking high", &mut context)
        .await
        .unwrap();
    assert_eq!(think_res, Some(CommandResult::Continue));
    assert_eq!(context.config.thinking_level.as_deref(), Some("high"));
}
