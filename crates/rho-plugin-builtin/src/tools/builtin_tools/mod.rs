pub mod catalog;
pub mod port;
#[cfg(test)]
mod tests;

pub use catalog::{
    BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS, PROMPT_ASK_USER, PROMPT_BASH, PROMPT_EDIT, PROMPT_READ,
    PROMPT_TODO, PROMPT_WEBFETCH, PROMPT_WEBSEARCH, PROMPT_WRITE,
};
pub(crate) use port::HostQuestionPort;

use crate::tools::ask_user::AskUserTool;
use crate::tools::bash::BashTool;
use crate::tools::edit::EditTool;
use crate::tools::read::ReadTool;
use crate::tools::todo::{TodoStore, TodoTool};
use crate::tools::types::ToolResult;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::WriteTool;
use async_trait::async_trait;
use rho_core::config::Config;
use rho_core::error::{AppError, Result};
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::{ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub struct BuiltinToolCatalog {
    capabilities: BTreeMap<CapabilityId, Arc<dyn ToolCapability>>,
}

impl BuiltinToolCatalog {
    pub fn new(base_dir: &Path, config: &Config) -> Result<Self> {
        Self::with_todo_store(base_dir, config, TodoStore::new())
    }

    pub fn with_todo_store(base_dir: &Path, config: &Config, todo_store: TodoStore) -> Result<Self> {
        let http = HttpClient::new(config.allow_private_network)?;
        let search = WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(config.search_min_interval_ms),
            WebSearchConfig {
                region: config.region.clone(),
                timeout_sec: config.search_timeout_sec,
            },
        );
        let fetch = WebFetchTool::new(
            http,
            FetchCache::new(60, 64),
            WebFetchConfig {
                timeout_sec: config.fetch_timeout_sec,
                max_bytes: config.fetch_max_bytes,
                default_limit: config.fetch_limit,
            },
        );
        let tools = vec![
            BuiltinTool::Read(ReadTool::new(base_dir)),
            BuiltinTool::Write(WriteTool::with_exclusions(
                base_dir,
                [&config.config_dir, &config.sessions_dir],
            )),
            BuiltinTool::Edit(EditTool::with_exclusions(
                base_dir,
                [&config.config_dir, &config.sessions_dir],
            )),
            BuiltinTool::Bash(BashTool::new(base_dir)),
            BuiltinTool::WebSearch(search.clone(), "websearch"),
            BuiltinTool::WebSearch(search, "web_search"),
            BuiltinTool::WebFetch(fetch.clone(), "webfetch"),
            BuiltinTool::WebFetch(fetch, "web_fetch"),
            BuiltinTool::AskUser(AskUserTool::new(), "ask_user"),
            BuiltinTool::AskUser(AskUserTool::new(), "ask_user_question"),
            BuiltinTool::Todo(TodoTool::new(todo_store)),
        ];
        let capabilities = tools
            .into_iter()
            .map(|tool| (tool.descriptor().id, Arc::new(tool) as Arc<dyn ToolCapability>))
            .collect();
        Ok(Self { capabilities })
    }

    pub fn descriptors() -> Vec<ToolDescriptor> {
        DECLARATIONS
            .iter()
            .copied()
            .map(BuiltinToolDeclaration::descriptor)
            .collect()
    }

    pub fn into_capabilities(self) -> BTreeMap<CapabilityId, Arc<dyn ToolCapability>> {
        self.capabilities
    }
}

enum BuiltinTool {
    Read(ReadTool),
    Write(WriteTool),
    Edit(EditTool),
    Bash(BashTool),
    WebSearch(WebSearchTool, &'static str),
    WebFetch(WebFetchTool, &'static str),
    AskUser(AskUserTool, &'static str),
    Todo(TodoTool),
}

impl BuiltinTool {
    fn name(&self) -> &'static str {
        match self {
            Self::Read(_) => "read",
            Self::Write(_) => "write",
            Self::Edit(_) => "edit",
            Self::Bash(_) => "bash",
            Self::WebSearch(_, name) => name,
            Self::WebFetch(_, name) => name,
            Self::AskUser(_, name) => name,
            Self::Todo(_) => "todo",
        }
    }
}

#[async_trait]
impl ToolCapability for BuiltinTool {
    fn descriptor(&self) -> ToolDescriptor {
        DECLARATIONS
            .iter()
            .find(|declaration| declaration.name == self.name())
            .copied()
            .unwrap()
            .descriptor()
    }

    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> std::result::Result<ToolInvocationResponse, rho_sdk::capability::CapabilityError> {
        let result = match self {
            Self::Read(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Write(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Edit(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Bash(tool) => {
                let args = parse(request.arguments)?;
                let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let stream_task = async {
                    while let Some(chunk) = chunk_rx.recv().await {
                        host.stream_chunk(&chunk);
                    }
                };
                let exec_task = tool.execute_streaming(args, move |chunk| {
                    let _ = chunk_tx.send(chunk.to_string());
                });
                let (result, _) = tokio::join!(exec_task, stream_task);
                result
            }
            Self::WebSearch(tool, _) => tool.execute(parse(request.arguments)?).await,
            Self::WebFetch(tool, _) => tool.execute(parse(request.arguments)?).await,
            Self::AskUser(tool, _) => {
                let port = HostQuestionPort(host);
                tool.execute(&port, parse(request.arguments)?).await
            }
            Self::Todo(tool) => tool.execute(parse(request.arguments)?).await,
        }
        .map_err(map_app_error)?;
        Ok(map_result(result))
    }
}

fn parse<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> std::result::Result<T, rho_sdk::capability::CapabilityError> {
    serde_json::from_value(value).map_err(|_| rho_sdk::capability::CapabilityError::InvalidRequest {
        message: "tool arguments do not match the declared schema".to_string(),
    })
}

fn map_result(result: ToolResult) -> ToolInvocationResponse {
    ToolInvocationResponse {
        content: result.content,
        is_error: result.is_error,
        structured_content: result.metadata,
    }
}

fn map_app_error(error: AppError) -> rho_sdk::capability::CapabilityError {
    match error {
        AppError::Cancelled(_) => rho_sdk::capability::CapabilityError::Cancelled,
        other => rho_sdk::capability::CapabilityError::Failed {
            message: other.to_string(),
        },
    }
}
