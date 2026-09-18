use rig::agent::hook::{
    AgentHook, CompletionCall, CompletionCallAction, HookContext, ModelTurnAction, ModelTurnFinished, RequestPatch,
};
use rig::completion::FinishReason;
use rig::message::AssistantContent;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub struct TruncationRecoveryHook {
    cap: AtomicU64,
    ceiling: u64,
    retries: AtomicUsize,
    max_retries: usize,
}

impl TruncationRecoveryHook {
    pub fn new(initial_cap: Option<u64>, ceiling: u64, max_retries: usize) -> Self {
        Self {
            cap: AtomicU64::new(initial_cap.unwrap_or(0)),
            ceiling,
            retries: AtomicUsize::new(0),
            max_retries,
        }
    }
}

impl AgentHook for TruncationRecoveryHook {
    async fn on_completion_call(&self, _ctx: &HookContext, _event: CompletionCall<'_>) -> CompletionCallAction {
        let cap = self.cap.load(Ordering::Relaxed);
        if cap > 0 {
            CompletionCallAction::patch(RequestPatch::new().max_tokens(cap))
        } else {
            CompletionCallAction::continue_run()
        }
    }

    async fn on_model_turn_finished(&self, _ctx: &HookContext, event: ModelTurnFinished<'_>) -> ModelTurnAction {
        let is_length_truncated = matches!(event.finish_reason, Some(FinishReason::Length));
        let has_tool_call = event
            .content
            .iter()
            .any(|content| matches!(content, AssistantContent::ToolCall(_)));
        let room = event.max_tokens.is_some_and(|cap| cap < self.ceiling);

        if is_length_truncated && !has_tool_call && room && self.retries.load(Ordering::Relaxed) < self.max_retries {
            self.retries.fetch_add(1, Ordering::Relaxed);
            let current = event.max_tokens.unwrap_or(self.ceiling);
            let grown = current.saturating_mul(2).min(self.ceiling);
            self.cap.store(grown, Ordering::Relaxed);
            return ModelTurnAction::repeat();
        }
        ModelTurnAction::continue_run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::agent::AgentBuilder;
    use rig::completion::Prompt;
    use rig::test_utils::{MockCompletionModel, MockTurn};

    #[tokio::test]
    async fn test_normal_finish_proceeds() {
        let hook = TruncationRecoveryHook::new(Some(1000), 8000, 2);
        let model = MockCompletionModel::new([MockTurn::text("done")]);
        let agent = AgentBuilder::new(model.clone())
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let resp = agent.prompt("hello").await.unwrap();
        assert_eq!(resp, "done");
        assert_eq!(model.requests().len(), 1);
    }

    #[tokio::test]
    async fn test_truncated_repeats_with_escalated_cap() {
        let hook = TruncationRecoveryHook::new(Some(1000), 8000, 2);
        let model = MockCompletionModel::new([
            MockTurn::text("cut off...").with_finish_reason(FinishReason::Length),
            MockTurn::text("completed successfully!").with_finish_reason(FinishReason::Stop),
        ]);
        let agent = AgentBuilder::new(model.clone())
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let resp = agent.runner("generate code").max_turns(3).run().await.unwrap();
        assert_eq!(resp.output, "completed successfully!");
        let reqs = model.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].max_tokens, Some(1000));
        assert_eq!(reqs[1].max_tokens, Some(2000));
    }

    #[tokio::test]
    async fn test_truncated_with_tool_call_does_not_repeat() {
        let hook = TruncationRecoveryHook::new(Some(1000), 8000, 2);
        let model = MockCompletionModel::new([
            MockTurn::tool_call("1", "test_tool", serde_json::json!({})).with_finish_reason(FinishReason::Length)
        ]);
        let agent = AgentBuilder::new(model.clone())
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let _ = agent.prompt("do tool").await;
        assert_eq!(model.requests().len(), 1);
    }
}
