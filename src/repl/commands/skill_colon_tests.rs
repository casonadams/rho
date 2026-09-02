#[cfg(test)]
mod tests {
    use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
    use crate::ui::TerminalRenderer;
    use crate::ui::interactive::{InteractiveUi, UiEvent};
    use rho_core::config::Config;
    use rho_engine::auth::AuthStore;
    use tokio::sync::mpsc;

    fn collecting_renderer() -> (TerminalRenderer, mpsc::UnboundedReceiver<UiEvent>) {
        let (ui, events) = InteractiveUi::channel();
        (TerminalRenderer::with_ui(ui), events)
    }

    #[tokio::test]
    async fn test_slash_skill_colon_invocation() {
        let workspace = std::env::temp_dir().join(format!("skill_colon_{}", uuid::Uuid::new_v4()));
        let config_dir = workspace.join("config");
        let user_skill_dir = config_dir.join("skills").join("my-flow");
        std::fs::create_dir_all(&user_skill_dir).unwrap();
        std::fs::write(
            user_skill_dir.join("SKILL.md"),
            "---\nname: my-flow\ndescription: Custom flow\n---\nRun step A then step B",
        )
        .unwrap();

        let mut config = Config {
            config_dir,
            ..Config::default()
        };
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            renderer: &renderer,
            session_id: None,
            session_manager: None,
        };

        let result = SlashCommandHandler::handle("/skill:my-flow create foo", &mut context)
            .await
            .unwrap();
        assert!(
            matches!(result, Some(CommandResult::ExpandedPrompt { text }) if text.contains("Run step A then step B") && text.contains("Skill input: create foo"))
        );

        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn test_new_and_thinking_commands() {
        let mut config = Config::default();
        let mut auth = AuthStore::default();
        let (renderer, _) = collecting_renderer();
        let mut context = SlashCommandContext {
            config: &mut config,
            auth_store: &mut auth,
            renderer: &renderer,
            session_id: None,
            session_manager: None,
        };

        let new_res = SlashCommandHandler::handle("/new", &mut context).await.unwrap();
        assert_eq!(new_res, Some(CommandResult::ClearContext));

        let think_res = SlashCommandHandler::handle("/thinking high", &mut context)
            .await
            .unwrap();
        assert_eq!(think_res, Some(CommandResult::Continue));
        assert_eq!(context.config.thinking_level.as_deref(), Some("high"));
    }
}
