use super::runner::{Round, RoundEnd, RoundInput, SubagentRunner};
use crate::engine::runner::history::without_unanswered_tool_calls;
use crate::tools::types::ToolResult;
use rho_harness_core::a2a::{Artifact, Message, Part, Role, Task, TaskState};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::{SessionManager, SubagentContext};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

/// What a subagent conversation keeps across tasks, stored as the A2A task metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConversationSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The agent file, for agents defined inside a workflow that the subagent tool does not know by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_file: Option<String>,
}

/// Subagent conversations with a round in progress; a second message to one of them is refused.
#[derive(Clone, Default)]
pub(crate) struct BusyContexts(Arc<Mutex<HashSet<String>>>);

pub(crate) struct BusyClaim {
    contexts: BusyContexts,
    context_id: String,
}

impl BusyContexts {
    pub(crate) fn claim(&self, context_id: &str) -> Result<BusyClaim, String> {
        let mut busy = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !busy.insert(context_id.to_string()) {
            return Err("that subagent is still working; wait for its result before sending another message".into());
        }
        Ok(BusyClaim {
            contexts: self.clone(),
            context_id: context_id.to_string(),
        })
    }
}

impl Drop for BusyClaim {
    fn drop(&mut self) {
        let mut busy = self.contexts.0.lock().unwrap_or_else(PoisonError::into_inner);
        busy.remove(&self.context_id);
    }
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

fn new_task(context_id: String, settings: &ConversationSettings) -> Task {
    let prefix = settings.name.as_deref().unwrap_or("subagent");
    Task::new(
        format!("{prefix}-{}", short_id()),
        context_id,
        serde_json::to_value(settings).ok(),
    )
}

fn task_settings(task: &Task) -> ConversationSettings {
    task.metadata
        .clone()
        .and_then(|m| serde_json::from_value(m).ok())
        .unwrap_or_default()
}

/// One message exchange with a subagent: the A2A task it belongs to and the subagent transcript it continues.
pub(crate) struct Exchange {
    task: Task,
    transcript: Vec<ChatMessage>,
    saved_len: usize,
    continues_question: bool,
}

impl Exchange {
    pub(crate) fn start(settings: &ConversationSettings) -> Self {
        Self {
            task: new_task(uuid::Uuid::new_v4().to_string(), settings),
            transcript: Vec::new(),
            saved_len: 0,
            continues_question: false,
        }
    }

    /// Continues `task_id`: the same task while it waits for input, otherwise a new task in its context.
    pub(crate) fn resume(context: SubagentContext, task_id: &str) -> Result<Self, String> {
        let referenced = context
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .ok_or_else(|| format!("unknown subagent task '{task_id}'"))?;
        let latest = context.latest_task().unwrap_or(referenced);
        let continues_question = referenced.status.state == TaskState::InputRequired && referenced.id == latest.id;
        let task = if continues_question {
            referenced.clone()
        } else {
            new_task(referenced.context_id.clone(), &task_settings(latest))
        };
        Ok(Self {
            task,
            saved_len: context.transcript.len(),
            transcript: context.transcript,
            continues_question,
        })
    }

    pub(crate) fn settings(&self) -> ConversationSettings {
        task_settings(&self.task)
    }

    pub(crate) fn context_id(&self) -> &str {
        &self.task.context_id
    }

    /// Records the parent's message on the task; `reference` links a new task to the one it follows up.
    fn send(&mut self, text: &str, reference: Option<&str>) {
        let mut message = Message::text(Role::User, &self.task, text);
        message.reference_task_ids.extend(reference.map(str::to_string));
        self.task.history.push(message);
        self.task.set_status(TaskState::Working, None);
    }

    pub(crate) async fn converse(
        self,
        runner: &SubagentRunner,
        session: &SessionManager,
        input: &str,
        reference: Option<&str>,
    ) -> ToolResult {
        self.exchange(runner, session, input, reference).await.render(runner)
    }

    /// Runs one round from the parent's message and saves the task with the subagent's new messages.
    pub(crate) async fn exchange(
        mut self,
        runner: &SubagentRunner,
        session: &SessionManager,
        input: &str,
        reference: Option<&str>,
    ) -> Outcome {
        self.send(input, reference.filter(|_| !self.continues_question));
        let round_input = if self.continues_question {
            RoundInput::Reply(input)
        } else {
            RoundInput::Prompt(input)
        };
        let transcript = std::mem::take(&mut self.transcript);
        let round = runner.run_round(transcript, round_input).await;
        self.finish(&round);
        let saved = session
            .record_subagent_task(&self.task, self.new_messages(&round))
            .await;
        Outcome {
            round,
            task: self.task,
            save_error: saved.err().map(|e| e.to_string()),
        }
    }

    fn finish(&mut self, round: &Round) {
        let (state, text) = match &round.end {
            RoundEnd::Answered(text) => (TaskState::Completed, Some(text.trim().to_string())),
            RoundEnd::Asked(question) => (TaskState::InputRequired, Some(question.clone())),
            RoundEnd::Stopped { .. } => (TaskState::InputRequired, round.body().ok()),
            RoundEnd::Partial(text) | RoundEnd::Failed(text) => (TaskState::Failed, Some(text.clone())),
            RoundEnd::Cancelled => (TaskState::Canceled, None),
        };
        let message = text.map(|text| Message::text(Role::Agent, &self.task, text));
        self.task.history.extend(message.clone());
        if let RoundEnd::Answered(text) = &round.end {
            self.add_artifact("result", Part::text(text.trim()));
        }
        if !round.changed_files.is_empty() {
            self.add_artifact("files-changed", Part::data(serde_json::json!(round.changed_files)));
        }
        self.task.set_status(state, message);
    }

    fn add_artifact(&mut self, name: &str, part: Part) {
        self.task.artifacts.retain(|a| a.name.as_deref() != Some(name));
        self.task.artifacts.push(Artifact {
            artifact_id: format!("{}-{name}", self.task.id),
            name: Some(name.to_string()),
            parts: vec![part],
        });
    }

    /// The transcript messages this round added, without tool calls left unanswered by an interruption.
    fn new_messages(&self, round: &Round) -> Vec<ChatMessage> {
        let kept = match round.end {
            RoundEnd::Asked(_) => round.transcript.len(),
            _ => without_unanswered_tool_calls(&round.transcript).len(),
        };
        round.transcript[self.saved_len.min(kept)..kept].to_vec()
    }
}

pub(crate) struct Outcome {
    pub round: Round,
    pub task: Task,
    pub save_error: Option<String>,
}

impl Outcome {
    /// The saved task id, which the main agent can continue with the subagent tool.
    pub(crate) fn continuable_id(&self) -> Option<&str> {
        self.save_error.is_none().then_some(self.task.id.as_str())
    }

    fn render(&self, runner: &SubagentRunner) -> ToolResult {
        let round = &self.round;
        let state = self.task.status.state;
        let footer = runner.footer(
            round,
            &format!(" \u{b7} task {} \u{b7} {}", self.task.id, state.label()),
        );
        let mut text = match round.body() {
            Ok(body) => runner.with_changed_files(&body, round),
            Err(error) => error,
        };
        match &self.save_error {
            Some(error) => text.push_str(&format!(
                "\n\nWarning: this subagent task was not saved ({error}), so it cannot be continued."
            )),
            None => text.push_str(&self.follow_up_hint(state)),
        }
        text.push_str(&footer);
        if matches!(round.end, RoundEnd::Failed(_) | RoundEnd::Cancelled) {
            ToolResult::error(text)
        } else {
            ToolResult::success(text)
        }
    }

    fn follow_up_hint(&self, state: TaskState) -> String {
        let id = &self.task.id;
        match state {
            TaskState::InputRequired => format!(
                "\n\nThe subagent is waiting. Reply with subagent {{\"task_id\": \"{id}\", \"prompt\": \"<answer or go-ahead>\"}}."
            ),
            _ => format!(
                "\n\nTo request fixes or more work with its full context, call subagent {{\"task_id\": \"{id}\", \"prompt\": \"<feedback>\"}}."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SubagentTool, runner::ASK_PARENT_TOOL};
    use super::*;
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
    use crate::tools::engine_tool::{EngineTool, TurnToolContext};
    use crate::tools::{GenericTool, ToolResult};
    use rho_harness_core::model::{AssistantContent, ToolCall, ToolFunction, UserContent};
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> Vec<ModelStreamEvent> {
        vec![ModelStreamEvent::ToolCall(ToolCall::new(
            format!("call-{name}"),
            ToolFunction::new(name, args),
        ))]
    }

    fn text(reply: &str) -> Vec<ModelStreamEvent> {
        vec![ModelStreamEvent::Text(reply.to_string())]
    }

    fn read_tool() -> Arc<dyn EngineTool> {
        Arc::new(GenericTool::new("read", "reads", json!({}), |_| async {
            Ok(ToolResult::success("fn hot() { loop {} }"))
        }))
    }

    fn bound_tool(adapter: &MockAdapter, session: Option<SessionManager>) -> Arc<dyn EngineTool> {
        let ctx = TurnToolContext {
            session,
            ..Default::default()
        };
        SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()])
            .bind_turn(&ctx)
            .unwrap()
    }

    fn task_id(content: &str) -> String {
        let after = content.rsplit_once(" \u{b7} task ").unwrap().1;
        after.split(' ').next().unwrap().to_string()
    }

    fn tool_results(messages: &[ChatMessage]) -> Vec<(String, String)> {
        messages
            .iter()
            .filter_map(|m| match m {
                ChatMessage::User { content } => Some(content),
                _ => None,
            })
            .flatten()
            .filter_map(|c| match c {
                UserContent::ToolResult(r) => Some((r.name.clone(), format!("{:?}", r.content))),
                _ => None,
            })
            .collect()
    }

    async fn ask(tool: &Arc<dyn EngineTool>, args: serde_json::Value) -> String {
        tool.execute(args).await.unwrap().content
    }

    #[tokio::test]
    async fn a_question_pauses_the_task_and_the_reply_resumes_it() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(dir.path(), None).unwrap();
        let adapter = MockAdapter::from_turns([
            call("read", json!({ "path": "src/hot.rs" })),
            call(
                ASK_PARENT_TOOL,
                json!({ "question": "Optimize for latency or memory?" }),
            ),
            text("Latency: hoisted the allocation out of hot()."),
        ]);
        let tool = bound_tool(&adapter, Some(session.clone()));

        let asked = ask(&tool, json!({ "name": "performanceAudit", "prompt": "Audit hot.rs" })).await;
        let id = task_id(&asked);
        assert!(
            asked.starts_with("Question for you: Optimize for latency or memory?"),
            "{asked}"
        );
        assert!(asked.contains("\u{b7} input-required: incomplete, 2 turns"), "{asked}");
        assert!(id.starts_with("performanceAudit-"), "{id}");

        let done = ask(&tool, json!({ "task_id": id, "prompt": "Latency." })).await;

        assert!(done.starts_with("Latency: hoisted the allocation"), "{done}");
        assert_eq!(task_id(&done), id, "a reply continues the same task");
        assert!(done.contains("\u{b7} completed: 1 turns"), "{done}");
        let last = adapter.requests().pop().unwrap();
        assert!(last.prompt.is_empty());
        let results = tool_results(&last.messages);
        assert_eq!(results.len(), 2, "{results:?}");
        assert_eq!(results[1].0, ASK_PARENT_TOOL);
        assert!(results[1].1.contains("Latency."));
        assert!(last.tools.iter().any(|t| t.name == ASK_PARENT_TOOL));

        let context = session.subagent_context(&id).await.unwrap();
        let task = context.latest_task().unwrap();
        assert_eq!(context.tasks.len(), 1);
        assert_eq!(task.status.state, TaskState::Completed);
        let roles: Vec<Role> = task.history.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::User, Role::Agent, Role::User, Role::Agent]);
        assert_eq!(task.artifacts[0].name.as_deref(), Some("result"));
    }

    #[tokio::test]
    async fn review_feedback_starts_a_new_task_that_keeps_the_subagent_history() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(dir.path(), None).unwrap();
        let adapter = MockAdapter::from_turns([
            call("read", json!({ "path": "src/hot.rs" })),
            text("Found one hot loop."),
        ]);
        let first = ask(
            &bound_tool(&adapter, Some(session.clone())),
            json!({ "name": "perf audit", "prompt": "Audit hot.rs" }),
        )
        .await;
        let first_id = task_id(&first);
        assert!(first.contains("call subagent {\"task_id\""), "{first}");
        let session_id = session.session_id.clone();
        drop(session);

        let reopened = SessionManager::new(dir.path(), Some(&session_id)).unwrap();
        adapter.add_turn(
            text("Also found the allocation in cold().")
                .into_iter()
                .map(Ok)
                .collect(),
        );
        let second = ask(
            &bound_tool(&adapter, Some(reopened.clone())),
            json!({ "task_id": first_id, "prompt": "You missed cold(); check it." }),
        )
        .await;

        let second_id = task_id(&second);
        assert_ne!(second_id, first_id, "a finished task is never restarted");
        assert!(second_id.starts_with("perfAudit-"), "{second_id}");
        let request = adapter.requests().pop().unwrap();
        assert_eq!(request.prompt, "You missed cold(); check it.");
        assert_eq!(request.messages[0], ChatMessage::user("Audit hot.rs"));
        assert_eq!(
            request.messages.last(),
            Some(&ChatMessage::assistant("Found one hot loop."))
        );
        let context = reopened.subagent_context(&second_id).await.unwrap();
        assert_eq!(context.tasks.len(), 2);
        assert_eq!(context.tasks[1].history[0].reference_task_ids, vec![first_id]);
        assert_eq!(context.transcript.len(), 6);
    }

    #[tokio::test]
    async fn a_turn_limit_waits_for_a_go_ahead_and_continues_with_its_history() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(dir.path(), None).unwrap();
        let adapter = MockAdapter::from_turns([
            call("read", json!({ "path": "a.rs" })),
            text("So far: a.rs is clean."),
            text("Finished: b.rs is clean too."),
        ]);
        let tool = bound_tool(&adapter, Some(session.clone()));

        let stopped = ask(&tool, json!({ "prompt": "Audit a.rs and b.rs", "turns": 1 })).await;
        assert!(
            stopped.starts_with("Incomplete: subagent reached the turn limit of 1"),
            "{stopped}"
        );
        assert!(stopped.contains("The subagent is waiting."), "{stopped}");
        let done = ask(&tool, json!({ "task_id": task_id(&stopped), "prompt": "continue" })).await;

        assert!(done.starts_with("Finished: b.rs is clean too."), "{done}");
        assert_eq!(task_id(&done), task_id(&stopped));
        let request = adapter.requests().pop().unwrap();
        assert_eq!(request.prompt, "continue");
        assert_eq!(
            request.messages.last(),
            Some(&ChatMessage::assistant("So far: a.rs is clean."))
        );
    }

    #[tokio::test]
    async fn bad_continuations_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(dir.path(), None).unwrap();
        let adapter = MockAdapter::from_turns([text("done")]);
        let tool = bound_tool(&adapter, Some(session));
        let id = task_id(&ask(&tool, json!({ "role": "scout", "prompt": "look" })).await);

        let cases = [
            (
                json!({ "task_id": "nope-1234", "prompt": "x" }),
                "unknown subagent task 'nope-1234'",
            ),
            (
                json!({ "task_id": id, "role": "general", "prompt": "x" }),
                "runs as 'scout', not 'general'",
            ),
        ];
        for (args, expected) in cases {
            let result = tool.execute(args).await.unwrap();
            assert!(result.is_error);
            assert!(result.content.contains(expected), "{}", result.content);
        }
        let without_session = ask(&bound_tool(&adapter, None), json!({ "task_id": id, "prompt": "x" })).await;
        assert!(without_session.contains("needs a saved session"), "{without_session}");
    }

    #[test]
    fn a_busy_conversation_refuses_a_second_message_until_the_first_finishes() {
        let busy = BusyContexts::default();
        let claim = busy.claim("ctx").unwrap();
        assert!(busy.claim("ctx").is_err());
        assert!(busy.claim("other").is_ok());
        drop(claim);
        assert!(busy.claim("ctx").is_ok());
    }

    #[tokio::test]
    async fn subagents_without_a_parent_session_get_no_ask_parent_tool() {
        let adapter = MockAdapter::from_turns([text("done")]);
        let result = ask(&bound_tool(&adapter, None), json!({ "prompt": "look" })).await;

        assert!(!result.contains("task "), "{result}");
        assert!(!adapter.requests()[0].tools.iter().any(|t| t.name == ASK_PARENT_TOOL));
    }

    #[test]
    fn only_open_questions_are_answered() {
        let asked = ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::ToolCall(ToolCall::new("q1", ToolFunction::new(ASK_PARENT_TOOL, json!({})))),
                AssistantContent::ToolCall(ToolCall::new("r1", ToolFunction::new("read", json!({})))),
            ],
        };
        let read_result = ChatMessage::User {
            content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                "r1", "read", "body",
            ))],
        };
        let mut transcript = vec![asked, read_result];

        assert!(super::super::runner::answer_pending_questions(&mut transcript, "yes"));
        assert!(!super::super::runner::answer_pending_questions(
            &mut transcript,
            "again"
        ));
        assert_eq!(tool_results(&transcript).len(), 2);
    }
}
