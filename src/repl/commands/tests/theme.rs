use super::{collected_output, collecting_renderer, test_context};
use crate::config::Config;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use rho_engine::auth::AuthStore;

#[tokio::test]
async fn theme_command_with_valid_theme_dispatches_change() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/theme nord", &mut context).await.unwrap();

    assert_eq!(
        result,
        Some(CommandResult::ThemeChanged {
            theme: "nord".to_string()
        })
    );
}

#[tokio::test]
async fn theme_command_with_unknown_theme_prints_error_and_available_list() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, mut events) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/theme non-existent-theme", &mut context)
        .await
        .unwrap();

    assert_eq!(result, Some(CommandResult::Continue));
    let output = collected_output(&mut events);
    assert!(output.contains("Unknown theme \"non-existent-theme\""));
    assert!(output.contains("Available themes:"));
    assert!(output.contains("nord"));
    assert!(output.contains("catppuccin"));
}

#[tokio::test]
async fn theme_command_without_args_opens_theme_selector_in_interactive_mode() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/theme", &mut context).await.unwrap();

    assert_eq!(result, Some(CommandResult::OpenThemeSelector));
}
