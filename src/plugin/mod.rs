pub mod activation;
pub mod builtin;
pub mod builtin_tools;
pub mod capability;
pub mod context;
pub mod contract;
pub mod extension;
pub mod external;
pub mod hook;
pub mod inspection;
pub mod loader;
pub mod permission;
pub mod process;
pub mod protocol;
pub mod provider;
pub mod registry;
pub mod resolver;
pub mod safety_floor;
pub mod schema;
pub mod tool_dispatch;
pub mod types;

pub use context::ExtensionContext;
pub use extension::Extension;
pub use hook::ExtensionHook;
pub use loader::{PluginDiscovery, PluginLoader};
pub use registry::ExtensionRegistry;
pub use types::{
    CommandHandler, CommandRequest, ExtensionCommand, InputAction, PluginCapability, PluginManifest, ToolCallDecision,
    ToolCallEvent, ToolResultEvent, TurnEvent,
};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct TestPlugin {
        name: String,
    }

    #[async_trait]
    impl Extension for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        async fn on_input(&self, input: &str, _ctx: &ExtensionContext) -> crate::error::Result<InputAction> {
            if input == "ping" {
                Ok(InputAction::Handled {
                    output: "pong".to_string(),
                })
            } else if input.starts_with("?rewrite ") {
                Ok(InputAction::Transform(input.replace("?rewrite ", "prefix: ")))
            } else {
                Ok(InputAction::Continue)
            }
        }

        async fn before_turn(&self, event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> crate::error::Result<()> {
            event.system_prompt.push_str("\nPlugin active.");
            Ok(())
        }

        async fn on_tool_call(
            &self,
            event: &ToolCallEvent<'_>,
            _ctx: &ExtensionContext,
        ) -> crate::error::Result<ToolCallDecision> {
            if event.tool_name == "forbidden_tool" {
                Ok(ToolCallDecision::Block {
                    reason: "Forbidden".to_string(),
                    terminate: false,
                })
            } else {
                Ok(ToolCallDecision::Allow)
            }
        }
    }

    struct EchoCommand;

    #[async_trait]
    impl CommandHandler for EchoCommand {
        async fn execute(&self, args: &str, _ctx: &ExtensionContext) -> crate::error::Result<String> {
            Ok(format!("Echo: {args}"))
        }
    }

    struct CommandPlugin;

    #[async_trait]
    impl Extension for CommandPlugin {
        fn name(&self) -> &str {
            "command_plugin"
        }

        fn register_commands(&self) -> Vec<ExtensionCommand> {
            vec![ExtensionCommand {
                name: "custom_echo".to_string(),
                description: "Echoes back the argument".to_string(),
                handler: Arc::new(EchoCommand),
            }]
        }
    }

    #[tokio::test]
    async fn test_input_lifecycle_handled() {
        let mut registry = ExtensionRegistry::new();
        registry.register(TestPlugin {
            name: "test".to_string(),
        });
        let ctx = ExtensionContext::new(".", "session_1");

        let action = registry.dispatch_input("ping", &ctx).await.unwrap();
        assert_eq!(
            action,
            InputAction::Handled {
                output: "pong".to_string()
            }
        );
    }

    #[tokio::test]
    async fn test_input_lifecycle_transform() {
        let mut registry = ExtensionRegistry::new();
        registry.register(TestPlugin {
            name: "test".to_string(),
        });
        let ctx = ExtensionContext::new(".", "session_1");

        let action = registry.dispatch_input("?rewrite hello", &ctx).await.unwrap();
        assert_eq!(action, InputAction::Transform("prefix: hello".to_string()));
    }

    #[tokio::test]
    async fn test_before_turn_augments_prompt() {
        let mut registry = ExtensionRegistry::new();
        registry.register(TestPlugin {
            name: "test".to_string(),
        });
        let ctx = ExtensionContext::new(".", "session_1");

        let mut system_prompt = "Initial prompt.".to_string();
        let mut event = TurnEvent {
            prompt: "do something",
            system_prompt: &mut system_prompt,
        };
        registry.dispatch_before_turn(&mut event, &ctx).await.unwrap();
        assert_eq!(system_prompt, "Initial prompt.\nPlugin active.");
    }

    #[tokio::test]
    async fn test_tool_call_blocking() {
        let mut registry = ExtensionRegistry::new();
        registry.register(TestPlugin {
            name: "test".to_string(),
        });
        let ctx = ExtensionContext::new(".", "session_1");

        let args = serde_json::json!({});
        let call_event = ToolCallEvent {
            tool_name: "read",
            args: &args,
        };
        let allowed = registry.dispatch_tool_call(&call_event, &ctx).await.unwrap();
        assert_eq!(allowed, ToolCallDecision::Allow);

        let blocked_event = ToolCallEvent {
            tool_name: "forbidden_tool",
            args: &args,
        };
        let blocked = registry.dispatch_tool_call(&blocked_event, &ctx).await.unwrap();
        assert!(matches!(blocked, ToolCallDecision::Block { .. }));
    }

    #[tokio::test]
    async fn test_custom_command_dispatch() {
        let mut registry = ExtensionRegistry::new();
        registry.register(CommandPlugin);
        let ctx = ExtensionContext::new(".", "session_1");

        assert!(registry.has_command("custom_echo"));
        let req = CommandRequest {
            name: "custom_echo",
            args: "test message",
        };
        let res = registry.dispatch_command(&req, &ctx).await.unwrap().unwrap();
        assert_eq!(res, "Echo: test message");
    }

    struct CrashingPlugin;

    #[async_trait]
    impl Extension for CrashingPlugin {
        fn name(&self) -> &str {
            "crashing_plugin"
        }

        async fn on_input(&self, _input: &str, _ctx: &ExtensionContext) -> crate::error::Result<InputAction> {
            panic!("Intentional plugin crash in on_input");
        }

        async fn before_turn(&self, _event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> crate::error::Result<()> {
            panic!("Intentional plugin crash in before_turn");
        }
    }

    #[tokio::test]
    async fn test_crashing_plugin_is_isolated_without_bringing_down_session() {
        let mut registry = ExtensionRegistry::new();
        registry.register(CrashingPlugin);
        registry.register(TestPlugin {
            name: "healthy_plugin".to_string(),
        });
        let ctx = ExtensionContext::new(".", "session_1");

        // Input dispatch should survive crashing plugin and allow healthy plugin to run
        let action = registry.dispatch_input("ping", &ctx).await.unwrap();
        assert_eq!(
            action,
            InputAction::Handled {
                output: "pong".to_string()
            }
        );

        // Turn dispatch should survive crashing plugin and augment prompt from healthy plugin
        let mut system_prompt = "Initial prompt.".to_string();
        let mut event = TurnEvent {
            prompt: "hello",
            system_prompt: &mut system_prompt,
        };
        let turn_res = registry.dispatch_before_turn(&mut event, &ctx).await;
        assert!(turn_res.is_ok());
        assert!(system_prompt.contains("Plugin active."));
    }

    struct CustomOAuthPlugin;

    #[async_trait]
    impl Extension for CustomOAuthPlugin {
        fn name(&self) -> &str {
            "custom_oauth_plugin"
        }

        async fn on_auth_login(&self, provider: &str, _ctx: &ExtensionContext) -> crate::error::Result<bool> {
            if provider == "enterprise-sso" {
                return Ok(true);
            }
            Ok(false)
        }
    }

    #[tokio::test]
    async fn test_plugin_custom_oauth_login_dispatch() {
        let mut registry = ExtensionRegistry::new();
        registry.register(CustomOAuthPlugin);
        let ctx = ExtensionContext::new(".", "session_1");

        assert!(registry.dispatch_login("enterprise-sso", &ctx).await.unwrap());
        assert!(!registry.dispatch_login("unknown-oauth", &ctx).await.unwrap());
    }
}
