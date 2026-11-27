//! `complete_task`: how a subagent finishes its task. The call's `result` is the report the
//! delegating agent receives; it is refused while a message or background work is outstanding.

use crate::engine::runner::SteeringQueueProvider;
use crate::engine::runner::turn::types::message_waiting;
use crate::mailbox::Mailbox;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::ToolResult;
use rho_harness_core::error::AppError;
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) const COMPLETE_TASK_TOOL: &str = "complete_task";
const DESCRIPTION: &str = "Finish the task and send your report to the delegating agent. The `result` is the only thing it receives: text you write outside this call is not passed on. Unless your task asks for a specific format, include a one-paragraph summary, findings with file:line references, files changed (if any), and open questions. Call it once, when the work is done. When the task asks for structured output, put that one JSON object in `data` (and a short summary in `result`) instead of writing JSON in text.";
const REPORT_SENT: &str = "Report sent to the delegating agent.";
const EMPTY_RESULT: &str = "Not completed: `result` is empty. Call complete_task again with your full report.";
const MESSAGE_WAITING: &str = "Not completed: a new message arrived and follows this result. Address it, then call complete_task again with your full report.";
const WORK_RUNNING: &str = "Not completed: background work you started is still running, and its result arrives as a message. Wait for it (call wait, or end your turn without tool calls), then call complete_task again.";

const DATA_NOT_OBJECT: &str = "Not completed: `data` must be one JSON object. Call complete_task again.";

/// What the subagent finished with: its report text and, for structured tasks, an A2A data part.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Report {
    pub text: String,
    pub data: Option<Value>,
}

/// The report the subagent finished with, if it called `complete_task` successfully.
pub(crate) type ReportSlot = Arc<Mutex<Option<Report>>>;

pub(crate) struct CompleteTaskTool {
    report: ReportSlot,
    inbox: Option<Mailbox>,
    steering: Option<Arc<dyn SteeringQueueProvider>>,
}

impl CompleteTaskTool {
    /// The tool, and where it keeps the report.
    pub(crate) fn with_report_slot() -> (Arc<dyn EngineTool>, ReportSlot) {
        let report = ReportSlot::default();
        let tool = Arc::new(Self {
            report: report.clone(),
            inbox: None,
            steering: None,
        });
        (tool, report)
    }

    fn outstanding(&self) -> Option<&'static str> {
        let inbox = self.inbox.as_ref()?;
        if message_waiting(inbox, self.steering.as_deref()) {
            return Some(MESSAGE_WAITING);
        }
        inbox.has_running_jobs().then_some(WORK_RUNNING)
    }
}

/// The report, taken once.
pub(crate) fn take_report(slot: &ReportSlot) -> Option<Report> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

#[async_trait::async_trait]
impl EngineTool for CompleteTaskTool {
    fn name(&self) -> &str {
        COMPLETE_TASK_TOOL
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "result": { "type": "string", "description": "Your complete report to the delegating agent" },
                "data": { "type": "object", "description": "Optional structured result: one JSON object, when the task asks for machine-readable output" }
            },
            "required": ["result"]
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        let result = args["result"].as_str().unwrap_or_default().trim();
        let data = args.get("data").filter(|d| !d.is_null());
        if data.is_some_and(|d| !d.is_object()) {
            return Ok(ToolResult::error(DATA_NOT_OBJECT));
        }
        if result.is_empty() && data.is_none() {
            return Ok(ToolResult::error(EMPTY_RESULT));
        }
        if let Some(refusal) = self.outstanding() {
            return Ok(ToolResult::error(refusal));
        }
        *self.report.lock().unwrap_or_else(PoisonError::into_inner) = Some(Report {
            text: result.to_string(),
            data: data.cloned(),
        });
        Ok(ToolResult {
            ends_turn: true,
            ..ToolResult::success(REPORT_SENT)
        })
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(Self {
            report: self.report.clone(),
            inbox: ctx.inbox.clone(),
            steering: ctx.steering.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::{Envelope, Kind, Mailboxes, Party};

    fn bound(inbox: &Mailbox) -> (Arc<dyn EngineTool>, ReportSlot) {
        let (tool, report) = CompleteTaskTool::with_report_slot();
        let ctx = TurnToolContext {
            inbox: Some(inbox.clone()),
            ..Default::default()
        };
        (tool.bind_turn(&ctx).unwrap(), report)
    }

    async fn complete(tool: &Arc<dyn EngineTool>, result: &str) -> ToolResult {
        tool.execute(serde_json::json!({ "result": result })).await.unwrap()
    }

    #[tokio::test]
    async fn a_report_is_kept_once_and_ends_the_turn() {
        let boxes = Mailboxes::default();
        let (tool, report) = bound(&boxes.open("task", None));

        let sent = complete(&tool, "  findings at a.rs:3  ").await;

        assert!(sent.ends_turn && !sent.is_error, "{}", sent.content);
        assert_eq!(sent.content, REPORT_SENT);
        assert_eq!(
            take_report(&report).map(|r| r.text).as_deref(),
            Some("findings at a.rs:3")
        );
        assert_eq!(take_report(&report), None, "taken once");
    }

    #[tokio::test]
    async fn structured_data_travels_with_the_report_and_must_be_an_object() {
        let boxes = Mailboxes::default();
        let (tool, report) = bound(&boxes.open("task", None));

        let bad = tool
            .execute(serde_json::json!({ "result": "x", "data": [1] }))
            .await
            .unwrap();
        let only_data = tool
            .execute(serde_json::json!({ "result": "", "data": { "findings": [] } }))
            .await
            .unwrap();

        assert!(bad.is_error && bad.content == DATA_NOT_OBJECT, "{}", bad.content);
        assert!(only_data.ends_turn && !only_data.is_error, "{}", only_data.content);
        let kept = take_report(&report).unwrap();
        assert_eq!(kept.data, Some(serde_json::json!({ "findings": [] })));
    }

    #[tokio::test]
    async fn completion_is_refused_while_the_report_is_empty_or_work_is_outstanding() {
        let boxes = Mailboxes::default();
        let _boss = boxes.open("boss", None);
        let inbox = boxes.open("task", Some("boss"));
        let (tool, report) = bound(&inbox);

        let empty = complete(&tool, "   ").await;
        boxes
            .send(Envelope::new(
                Party::Agent("boss".into()),
                "task",
                Kind::Command,
                "one more thing",
            ))
            .unwrap();
        let messaged = complete(&tool, "done").await;
        assert_eq!(inbox.take().len(), 1, "the message stays for delivery");
        let _job = inbox.start_job("j1".into(), "cargo test".into());
        let working = complete(&tool, "done").await;

        for (refused, reason) in [
            (empty, EMPTY_RESULT),
            (messaged, MESSAGE_WAITING),
            (working, WORK_RUNNING),
        ] {
            assert!(refused.is_error && !refused.ends_turn, "{}", refused.content);
            assert_eq!(refused.content, reason);
        }
        assert_eq!(take_report(&report), None);
    }
}
