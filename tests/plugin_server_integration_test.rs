use rho_host::external::ExternalPlugin;
use rho_host::process::ProcessLimits;
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::{
    CommandCapability, CommandInvocationRequest, ContextCapability, ContextRequest, InteractionRequest,
    InteractionResponse, InvocationContext, LifecycleCapability, LifecycleEvent, ToolCapability, ToolHost,
    ToolInvocationRequest,
};
use std::path::PathBuf;

struct NoopToolHost;

#[async_trait::async_trait]
impl ToolHost for NoopToolHost {
    async fn interact(
        &self,
        _request: InteractionRequest,
    ) -> Result<InteractionResponse, rho_sdk::capability::CapabilityError> {
        Err(rho_sdk::capability::CapabilityError::Unavailable {
            message: "interaction unavailable".to_string(),
        })
    }
}

fn example_binary_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|s| s.to_str()) == Some("deps") {
        path.pop();
    }
    path.join("examples").join("context_rag_plugin")
}

#[tokio::test]
async fn test_context_rag_plugin_subprocess_end_to_end() {
    let executable = example_binary_path();
    let plugin = ExternalPlugin::load(&executable, ProcessLimits::default())
        .await
        .expect("Plugin handshake and discovery failed");

    assert_eq!(plugin.manifest().plugin_id.as_str(), "rho-plugin-docs");
    assert_eq!(plugin.manifest().plugin_version, "0.1.0");

    let context_id: CapabilityId = "context:docs".parse().unwrap();
    let context_cap = plugin.context(&context_id).expect("context capability missing");
    let ctx_res = context_cap
        .retrieve(ContextRequest {
            prompt: "Tell me about architecture and docs".to_string(),
            context: InvocationContext::new("s1", ".", true),
            token_budget: Some(2048),
        })
        .await
        .expect("context retrieval failed");

    assert_eq!(ctx_res.snippets.len(), 1);
    assert_eq!(ctx_res.snippets[0].source, "sqlite-vec://docs/architecture.md");
    assert!(ctx_res.snippets[0].content.contains("Project documentation indexed"));

    let command_id: CapabilityId = "command:docs".parse().unwrap();
    let command_cap = plugin.command(&command_id).expect("command capability missing");
    let cmd_res = command_cap
        .invoke(CommandInvocationRequest {
            arguments: vec!["index".to_string(), "./docs".to_string()],
            context: InvocationContext::new("s1", ".", true),
        })
        .await
        .expect("command invocation failed");

    assert_eq!(cmd_res.exit_code, 0);
    assert!(cmd_res.output.contains("42 files parsed"));

    let tool_id: CapabilityId = "tool:doc_search".parse().unwrap();
    let tool_cap = plugin.tool(&tool_id).expect("tool capability missing");
    let tool_res = tool_cap
        .invoke(
            &NoopToolHost,
            ToolInvocationRequest {
                arguments: serde_json::json!({"query": "architecture specs"}),
                context: InvocationContext::new("s1", ".", true),
            },
        )
        .await
        .expect("tool invocation failed");

    assert!(!tool_res.is_error);
    assert!(tool_res.content.contains("Matched 1 chunk in documentation index"));

    let lifecycle_id: CapabilityId = "lifecycle:docs".parse().unwrap();
    let lifecycle_cap = plugin.lifecycle(&lifecycle_id).expect("lifecycle capability missing");
    lifecycle_cap
        .notify(LifecycleEvent::AfterTurn {
            session_id: "s1".to_string(),
            success: true,
            files_modified: vec!["docs/architecture.md".to_string()],
        })
        .await
        .expect("lifecycle notification failed");
}
