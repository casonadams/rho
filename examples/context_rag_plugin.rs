use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{
    CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse, ContextCapability,
    ContextDescriptor, ContextRequest, ContextResponse, ContextSnippet, ExecutionMode, LifecycleCapability,
    LifecycleEvent, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use rho_sdk::{PluginBuilder, run};

struct LocalDocsContext;

#[async_trait]
impl ContextCapability for LocalDocsContext {
    fn descriptor(&self) -> ContextDescriptor {
        ContextDescriptor {
            id: "context:docs".parse().unwrap(),
            display_name: "Local Documentation Memory".to_string(),
            description: "Semantic retrieval over project documentation".to_string(),
            max_snippets: Some(3),
        }
    }

    async fn retrieve(&self, request: ContextRequest) -> Result<ContextResponse, CapabilityError> {
        let mut snippets = Vec::new();
        if request.prompt.to_lowercase().contains("doc")
            || request.prompt.to_lowercase().contains("architecture")
            || request.prompt.to_lowercase().contains("rag")
        {
            snippets.push(ContextSnippet {
                source: "sqlite-vec://docs/architecture.md".to_string(),
                title: Some("Project Architecture & Documentation".to_string()),
                content: "Project documentation indexed into local vector database for semantic retrieval.".to_string(),
                score: Some(0.92),
            });
        }
        Ok(ContextResponse { snippets })
    }
}

struct DocSearchTool;

#[async_trait]
impl ToolCapability for DocSearchTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "tool:doc_search".parse().unwrap(),
            description: "Search local documentation for semantic matches".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language search query"
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            prompt_guidance: "Use this tool to find relevant documentation and architecture references.".to_string(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Parallel,
        }
    }

    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        host.progress("Querying local vector embeddings...");
        let query = request
            .arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let result = format!("Matched 1 chunk in documentation index for query '{query}'");
        Ok(ToolInvocationResponse {
            content: result,
            is_error: false,
            structured_content: None,
        })
    }
}

struct DocsCommand;

#[async_trait]
impl CommandCapability for DocsCommand {
    fn descriptor(&self) -> CommandDescriptor {
        CommandDescriptor {
            id: "command:docs".parse().unwrap(),
            name: "docs".to_string(),
            description: "Manage and inspect local documentation index".to_string(),
        }
    }

    async fn invoke(&self, request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError> {
        let subcmd = request.arguments.first().map(String::as_str).unwrap_or("status");
        let output = match subcmd {
            "index" | "fire" => "Index pipeline completed: 42 files parsed, 128 chunks vectorized.".to_string(),
            "sync" => "Incremental sync completed: 0 changes detected.".to_string(),
            "search" => format!("Search results for: {}", request.arguments[1..].join(" ")),
            _ => "Local Documentation index status: Ready.".to_string(),
        };
        Ok(CommandInvocationResponse { output, exit_code: 0 })
    }
}

struct DocsLifecycle;

#[async_trait]
impl LifecycleCapability for DocsLifecycle {
    fn id(&self) -> CapabilityId {
        "lifecycle:docs".parse().unwrap()
    }

    async fn notify(&self, _event: LifecycleEvent) -> Result<(), CapabilityError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let plugin = PluginBuilder::new("rho-plugin-docs", "0.1.0")
        .context(LocalDocsContext)
        .tool(DocSearchTool)
        .command(DocsCommand)
        .lifecycle(DocsLifecycle)
        .build()?;

    run(plugin).await
}
