use crate::intent::{IntentHandle, IntentProgress};
use crate::tools::types::generated_schema;
use rig::tool::{Tool, ToolContext, ToolExecutionError};

#[derive(Clone, Default)]
pub struct IntentProgressTool;

impl Tool for IntentProgressTool {
    const NAME: &'static str = "intent_progress";
    type Args = IntentProgress;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Report completed outcomes, verification results, blocking reasons, and whether the active IntentSpec is complete. Call before finalizing a task.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<IntentProgress>()
    }

    async fn call(&self, context: &mut ToolContext, progress: Self::Args) -> Result<Self::Output, Self::Error> {
        let handle = context
            .get::<IntentHandle>()
            .ok_or_else(|| ToolExecutionError::other("Active IntentSpec context is missing"))?;
        handle
            .report_progress(progress)
            .map_err(ToolExecutionError::from_error)?;
        let state = handle.snapshot().map_err(ToolExecutionError::from_error)?;
        Ok(format!("Intent status: {:?}", state.status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{IntentSpec, VerificationResult};
    use rig::tool::ToolContext;

    #[tokio::test]
    async fn tool_updates_shared_intent_context() {
        let dir = std::env::temp_dir().join(format!("intent_progress_{}", uuid::Uuid::new_v4()));
        let handle = IntentHandle::create(
            &dir,
            crate::intent::NewIntent {
                spec: IntentSpec::from_prompt("finish task"),
                workspace: "/repo".to_string(),
                session_id: "session-1".to_string(),
                secrets: Vec::new(),
            },
        )
        .unwrap();
        let mut context = ToolContext::new();
        context.insert(handle.clone());

        IntentProgressTool
            .call(
                &mut context,
                IntentProgress {
                    completed_outcomes: Vec::new(),
                    verification: Vec::<VerificationResult>::new(),
                    blocked_reason: None,
                    complete: true,
                },
            )
            .await
            .unwrap();

        assert_eq!(handle.snapshot().unwrap().status, crate::intent::IntentStatus::Ready);
    }
}
