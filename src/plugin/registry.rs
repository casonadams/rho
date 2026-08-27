use crate::error::{AppError, Result};
use crate::plugin::context::ExtensionContext;
use crate::plugin::extension::Extension;
use crate::plugin::types::{
    CommandRequest, ExtensionCommand, InputAction, PluginCapability, ToolCallDecision, ToolCallEvent, ToolResultEvent,
    TurnEvent,
};
use futures::FutureExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const PLUGIN_HOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Default, Clone)]
pub struct ExtensionRegistry {
    extensions: Vec<Arc<dyn Extension>>,
    commands: HashMap<String, Arc<ExtensionCommand>>,
    audit: Arc<Mutex<Vec<String>>>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, extension: impl Extension + 'static) {
        self.register_arc(Arc::new(extension));
    }

    pub fn register_arc(&mut self, extension: Arc<dyn Extension>) {
        for cmd in extension.register_commands() {
            self.commands.insert(cmd.name.clone(), Arc::new(cmd));
        }
        self.extensions.push(extension);
        self.extensions.sort_by(|left, right| left.name().cmp(right.name()));
    }

    pub fn audit_events(&self) -> Vec<String> {
        self.audit.lock().map(|events| events.clone()).unwrap_or_default()
    }

    fn record_audit(&self, event: String) {
        if let Ok(mut events) = self.audit.lock() {
            events.push(event);
        }
    }

    pub fn extensions(&self) -> &[Arc<dyn Extension>] {
        &self.extensions
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    pub fn list_commands(&self) -> Vec<(&str, &str)> {
        let mut list: Vec<(&str, &str)> = self
            .commands
            .values()
            .map(|cmd| (cmd.name.as_str(), cmd.description.as_str()))
            .collect();
        list.sort_by_key(|(name, _)| *name);
        list
    }

    fn has_capability(ext: &dyn Extension, capability: PluginCapability) -> bool {
        ext.capabilities().contains(&capability)
    }

    pub async fn dispatch_command(&self, req: &CommandRequest<'_>, ctx: &ExtensionContext) -> Option<Result<String>> {
        let cmd = self.commands.get(req.name)?.clone();
        let res = std::panic::AssertUnwindSafe(cmd.handler.execute(req.args, ctx))
            .catch_unwind()
            .await;
        match res {
            Ok(result) => Some(result),
            Err(_) => Some(Err(AppError::Plugin(format!(
                "Command '/{}' panicked during execution",
                req.name
            )))),
        }
    }

    pub async fn dispatch_session_start(&self, ctx: &ExtensionContext) -> Result<()> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Lifecycle) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_session_start(ctx))
                .catch_unwind()
                .await;
            if let Ok(Err(err)) = res {
                eprintln!("Warning: plugin '{}' failed in on_session_start: {err}", ext.name());
            } else if res.is_err() {
                eprintln!("Warning: plugin '{}' panicked in on_session_start", ext.name());
            }
        }
        Ok(())
    }

    pub async fn dispatch_session_shutdown(&self, ctx: &ExtensionContext) -> Result<()> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Lifecycle) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_session_shutdown(ctx))
                .catch_unwind()
                .await;
            if let Ok(Err(err)) = res {
                eprintln!("Warning: plugin '{}' failed in on_session_shutdown: {err}", ext.name());
            } else if res.is_err() {
                eprintln!("Warning: plugin '{}' panicked in on_session_shutdown", ext.name());
            }
        }
        Ok(())
    }

    pub async fn dispatch_input(&self, input: &str, ctx: &ExtensionContext) -> Result<InputAction> {
        let mut current_input = input.to_string();
        let mut transformed = false;

        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Input) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_input(&current_input, ctx))
                .catch_unwind()
                .await;
            match res {
                Ok(Ok(InputAction::Continue)) => {}
                Ok(Ok(InputAction::Transform(new_text))) => {
                    current_input = new_text;
                    transformed = true;
                }
                Ok(Ok(InputAction::Handled { output })) => {
                    return Ok(InputAction::Handled { output });
                }
                Ok(Err(err)) => {
                    eprintln!("Warning: plugin '{}' failed in on_input: {err}", ext.name());
                }
                Err(_) => {
                    eprintln!("Warning: plugin '{}' panicked in on_input", ext.name());
                }
            }
        }

        if transformed {
            Ok(InputAction::Transform(current_input))
        } else {
            Ok(InputAction::Continue)
        }
    }

    pub async fn dispatch_before_turn(&self, event: &mut TurnEvent<'_>, ctx: &ExtensionContext) -> Result<()> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Lifecycle) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.before_turn(event, ctx))
                .catch_unwind()
                .await;
            match res {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    eprintln!("Warning: plugin '{}' failed in before_turn: {err}", ext.name());
                }
                Err(_) => {
                    eprintln!("Warning: plugin '{}' panicked in before_turn", ext.name());
                }
            }
        }
        Ok(())
    }

    pub async fn dispatch_tool_call(
        &self,
        event: &ToolCallEvent<'_>,
        ctx: &ExtensionContext,
    ) -> Result<ToolCallDecision> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::ToolCalls) {
                continue;
            }
            let res = tokio::time::timeout(
                PLUGIN_HOOK_TIMEOUT,
                std::panic::AssertUnwindSafe(ext.on_tool_call(event, ctx)).catch_unwind(),
            )
            .await;
            match res {
                Ok(Ok(Ok(ToolCallDecision::Block { reason, terminate }))) => {
                    self.record_audit(format!("tool_call:block:{}/{}", event.tool_name, reason));
                    return Ok(ToolCallDecision::Block { reason, terminate });
                }
                Ok(Ok(Ok(ToolCallDecision::Allow))) => {}
                Ok(Ok(Err(err))) => eprintln!("Warning: plugin '{}' failed in on_tool_call: {err}", ext.name()),
                Ok(Err(_)) => eprintln!("Warning: plugin '{}' panicked in on_tool_call", ext.name()),
                Err(_) => eprintln!("Warning: plugin '{}' exceeded on_tool_call timeout", ext.name()),
            }
        }
        Ok(ToolCallDecision::Allow)
    }

    pub async fn dispatch_tool_result(&self, event: &mut ToolResultEvent<'_>, ctx: &ExtensionContext) -> Result<()> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::ToolCalls) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_tool_result(event, ctx))
                .catch_unwind()
                .await;
            match res {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    eprintln!("Warning: plugin '{}' failed in on_tool_result: {err}", ext.name());
                }
                Err(_) => {
                    eprintln!("Warning: plugin '{}' panicked in on_tool_result", ext.name());
                }
            }
        }
        Ok(())
    }

    pub async fn dispatch_login(&self, provider: &str, ctx: &ExtensionContext) -> Result<bool> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Authentication) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_auth_login(provider, ctx))
                .catch_unwind()
                .await;
            match res {
                Ok(Ok(true)) => return Ok(true),
                Ok(Ok(false)) => {}
                Ok(Err(err)) => {
                    eprintln!("Warning: plugin '{}' failed during login: {err}", ext.name());
                }
                Err(_) => {
                    eprintln!("Warning: plugin '{}' panicked during login", ext.name());
                }
            }
        }
        Ok(false)
    }

    pub async fn dispatch_logout(&self, provider: &str, ctx: &ExtensionContext) -> Result<bool> {
        for ext in &self.extensions {
            if !Self::has_capability(ext.as_ref(), PluginCapability::Authentication) {
                continue;
            }
            let res = std::panic::AssertUnwindSafe(ext.on_auth_logout(provider, ctx))
                .catch_unwind()
                .await;
            match res {
                Ok(Ok(true)) => return Ok(true),
                Ok(Ok(false)) => {}
                Ok(Err(err)) => {
                    eprintln!("Warning: plugin '{}' failed during logout: {err}", ext.name());
                }
                Err(_) => {
                    eprintln!("Warning: plugin '{}' panicked during logout", ext.name());
                }
            }
        }
        Ok(false)
    }
}
