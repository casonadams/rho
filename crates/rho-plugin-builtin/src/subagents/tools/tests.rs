use super::*;
use crate::subagents::runner::{NoopExecutor, SubagentRunner};
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{ToolHost, ToolInvocationRequest};

struct DummyHost;
#[async_trait::async_trait]
impl ToolHost for DummyHost {
    async fn interact(
        &self,
        _request: rho_sdk::contract::InteractionRequest,
    ) -> std::result::Result<rho_sdk::contract::InteractionResponse, CapabilityError> {
        unreachable!()
    }

    fn stream_chunk(&self, _chunk: &str) {}
}

#[tokio::test]
async fn test_subagent_tool_spawns_background_and_polls_result() {
    let runner = Arc::new(SubagentRunner::new(Arc::new(NoopExecutor), 10));
    let supervisor = SubagentSupervisor::new(runner, 4);
    let config = Config::default();
    let tools = create_subagent_tools(supervisor.clone(), &config, Path::new("."));
    assert_eq!(tools.len(), 3);

    let agent_tool = &tools[0].1;
    let get_result_tool = &tools[1].1;
    let host = DummyHost;

    let res = agent_tool
        .invoke(
            &host,
            ToolInvocationRequest {
                arguments: serde_json::json!({
                    "subagent_type": "explore",
                    "prompt": "search auth files",
                    "run_in_background": true
                }),
                context: rho_sdk::contract::InvocationContext::new("test", ".", false),
            },
        )
        .await
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&res.content).unwrap();
    let job_id = parsed["job_id"].as_str().unwrap();

    // Wait for background job to settle
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let poll_res = get_result_tool
        .invoke(
            &host,
            ToolInvocationRequest {
                arguments: serde_json::json!({ "agent_id": job_id }),
                context: rho_sdk::contract::InvocationContext::new("test", ".", false),
            },
        )
        .await
        .unwrap();

    assert!(!poll_res.is_error);
}
