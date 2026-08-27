use crate::error::Result;
use crate::plugin::context::ExtensionContext;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    Lifecycle,
    Input,
    ToolCalls,
    Commands,
    Authentication,
}

impl PluginCapability {
    pub const ALL: [Self; 5] = [
        Self::Lifecycle,
        Self::Input,
        Self::ToolCalls,
        Self::Commands,
        Self::Authentication,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputAction {
    Continue,
    Transform(String),
    Handled { output: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallDecision {
    Allow,
    Block { reason: String, terminate: bool },
}

#[derive(Debug)]
pub struct TurnEvent<'a> {
    pub prompt: &'a str,
    pub system_prompt: &'a mut String,
}

#[derive(Debug)]
pub struct ToolCallEvent<'a> {
    pub tool_name: &'a str,
    pub args: &'a serde_json::Value,
}

#[derive(Debug)]
pub struct ToolResultEvent<'a> {
    pub tool_name: &'a str,
    pub args: &'a serde_json::Value,
    pub result: &'a mut String,
}

#[derive(Debug)]
pub struct CommandRequest<'a> {
    pub name: &'a str,
    pub args: &'a str,
}

#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn execute(&self, args: &str, ctx: &ExtensionContext) -> Result<String>;
}

pub struct ExtensionCommand {
    pub name: String,
    pub description: String,
    pub handler: Arc<dyn CommandHandler>,
}

impl Debug for ExtensionCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

fn default_api_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub entrypoint: Option<String>,
    pub wasm_binary: Option<String>,
    #[serde(default = "default_api_version")]
    pub api_version: u32,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
}
