use std::sync::Arc;

use rig::agent::hook::{AgentHook, HookContext};
use rig::agent::{AgentBuilder, ModelHandle};
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;

use super::super::TurnToolExecutionHook;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::runner::turn::types::{ActiveModelSwitch, SharedModelSwitch};
use rho_harness_core::session::SessionManager;

fn mock_sink() -> Arc<TerminalApprovalSink> {
    let temp_dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
    let session = SessionManager::new(&temp_dir, None).unwrap();
    TerminalApprovalSink::new(
        &crate::engine::eval::presenter::presenter(),
        TerminalSinkConfig {
            model_label: "test-model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    )
}

#[tokio::test]
async fn test_shared_model_switch_state_transitions() {
    let switcher = SharedModelSwitch::new();
    assert!(switcher.get_handle().is_none());
    assert!(switcher.current_model().is_none());
    assert!(switcher.current_provider().is_none());
    assert!(switcher.take_switched().is_none());

    let mock = MockCompletionModel::text("test");
    let handle = ModelHandle::new(mock);
    switcher.switch_to(ActiveModelSwitch::new("gemini-2.5-pro", "gemini", handle));

    assert_eq!(switcher.current_model().as_deref(), Some("gemini-2.5-pro"));
    assert_eq!(switcher.current_provider().as_deref(), Some("gemini"));
    assert!(switcher.get_handle().is_some());
    assert_eq!(
        switcher.take_switched(),
        Some(("gemini-2.5-pro".to_string(), "gemini".to_string()))
    );
}

#[tokio::test]
async fn test_runtime_model_switching_across_turns_within_agent() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("out.txt");

    let switcher = Arc::new(SharedModelSwitch::new());
    let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", None).with_model_switch(Some(switcher.clone()));

    let model_1 = MockCompletionModel::new([MockTurn::tool_call(
        "1",
        "write",
        json!({"path": file, "content": "first model wrote"}),
    )]);

    let model_2 = MockCompletionModel::new([MockTurn::text("second model finished")]);
    let handle_2 = ModelHandle::new(model_2.clone());

    let switcher_clone = switcher.clone();
    let handle_2_clone = handle_2.clone();

    struct SwitchHook {
        switcher: Arc<SharedModelSwitch>,
        next_model: ModelHandle,
    }
    impl AgentHook for SwitchHook {
        async fn on_tool_result(
            &self,
            _ctx: &HookContext,
            _event: rig::agent::hook::ToolResultEvent<'_>,
        ) -> rig::agent::hook::ToolResultAction {
            self.switcher
                .switch_to(ActiveModelSwitch::new("model-2", "mock", self.next_model.clone()));
            rig::agent::hook::ToolResultAction::keep()
        }
    }

    let agent = AgentBuilder::new(model_1.clone())
        .tool(crate::tools::WriteTool::new(dir.path()))
        .add_hook(hook)
        .add_hook(SwitchHook {
            switcher: switcher_clone,
            next_model: handle_2_clone,
        })
        .record_content_telemetry(false)
        .build();

    let response = agent.runner("execute task").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "second model finished");
    assert_eq!(model_1.requests().len(), 1);
    assert_eq!(model_2.requests().len(), 1);
}
