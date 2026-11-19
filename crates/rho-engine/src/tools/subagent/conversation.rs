use super::handoff::{WORKTREE_ARTIFACT, WorktreeRecord, changed_since, moved_note, pending_note, settle};
use super::runner::{Round, RoundEnd, RoundInput, SubagentRunner};
use super::workspace::{Fingerprint, WorkspaceChange, stale_paths, versions};
use crate::engine::runner::history::without_unanswered_tool_calls;
use crate::tools::types::ToolResult;
use crate::worktree::{Worktree, WorktreeLease, WorktreeStore};
use rho_harness_core::a2a::{Artifact, Message, Part, Role, Task, TaskState};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::{SessionManager, SubagentContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) const RESULT_ARTIFACT: &str = "result";
pub(crate) const FILES_CHANGED_ARTIFACT: &str = "files-changed";
pub(crate) const WORKSPACE_CHANGES_ARTIFACT: &str = "workspace-changes";
pub(crate) const TOOL_CALLS_ARTIFACT: &str = "tool-calls";
/// Content fingerprints of the files the subagent has read or edited, checked when it is continued.
pub(crate) const FILE_VERSIONS_ARTIFACT: &str = "file-versions";
const MAX_NOTED_OTHERS: usize = 20;

pub(crate) fn artifact_data<'a>(task: &'a Task, name: &str) -> Option<&'a Value> {
    task.artifacts
        .iter()
        .find(|a| a.name.as_deref() == Some(name))
        .and_then(|a| a.parts.iter().find_map(|p| p.data.as_ref()))
}

pub(crate) fn artifact_text<'a>(task: &'a Task, name: &str) -> Option<&'a str> {
    task.artifacts
        .iter()
        .find(|a| a.name.as_deref() == Some(name))
        .and_then(|a| a.parts.iter().find_map(|p| p.text.as_deref()))
}

fn file_versions(task: &Task) -> BTreeMap<String, Fingerprint> {
    artifact_data(task, FILE_VERSIONS_ARTIFACT)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub(crate) fn worktree_record(task: &Task) -> Option<WorktreeRecord> {
    artifact_data(task, WORKTREE_ARTIFACT).and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn others_note(others: &[WorkspaceChange]) -> String {
    if others.is_empty() {
        return String::new();
    }
    let mut listed: Vec<String> = others
        .iter()
        .take(MAX_NOTED_OTHERS)
        .map(WorkspaceChange::label)
        .collect();
    if others.len() > MAX_NOTED_OTHERS {
        listed.push(format!("and {} more", others.len() - MAX_NOTED_OTHERS));
    }
    format!(
        "[Other files changed in the workspace since your last turn: {}.]\n\n",
        listed.join(", ")
    )
}

fn stale_text(stale: &[String]) -> String {
    if stale.is_empty() {
        return String::new();
    }
    format!(
        "[These files changed since you last read or edited them; re-read them before relying on what you saw earlier: {}]\n\n",
        stale.join(", ")
    )
}

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
    versions: BTreeMap<String, Fingerprint>,
    /// The worktree recorded at the end of the conversation's last round.
    previous: Option<WorktreeRecord>,
    worktree: Option<WorktreeLease>,
}

impl Exchange {
    pub(crate) fn start(settings: &ConversationSettings) -> Self {
        Self {
            task: new_task(uuid::Uuid::new_v4().to_string(), settings),
            transcript: Vec::new(),
            saved_len: 0,
            continues_question: false,
            versions: BTreeMap::new(),
            previous: None,
            worktree: None,
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
            versions: file_versions(latest),
            previous: worktree_record(latest),
            task,
            saved_len: context.transcript.len(),
            transcript: context.transcript,
            continues_question,
            worktree: None,
        })
    }

    /// Gives the round a worktree owned by session `owner`: the conversation's existing one if it still
    /// has unmerged changes, otherwise a new one from `source`. Outside git the agent runs in `source`.
    pub(crate) async fn with_worktree(
        mut self,
        store: &WorktreeStore,
        source: &std::path::Path,
        label: &str,
        owner: Option<&str>,
    ) -> Result<Self, String> {
        let existing = match self.previous.as_ref().filter(|p| !p.removed) {
            Some(record) => store.find(source, &record.id).await,
            None => None,
        };
        let worktree = match existing {
            Some(worktree) => Some(worktree),
            None => store.create(source, label, owner).await?,
        };
        self.worktree = worktree.map(WorktreeLease::new);
        Ok(self)
    }

    fn current_worktree(&self) -> Option<&Worktree> {
        self.worktree.as_ref().map(WorktreeLease::worktree)
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
        let runner = match self.current_worktree() {
            Some(worktree) => runner.clone().in_worktree(&worktree.path),
            None => runner.clone(),
        };
        let root = runner.workspace_root();
        let stale = stale_paths(&root, &self.versions).await;
        let others = self.other_changes(&root, &stale).await;
        let noted = format!(
            "{}{}{}{input}",
            self.moved_text(),
            stale_text(&stale),
            others_note(&others)
        );
        let round_input = if self.continues_question {
            RoundInput::Reply(&noted)
        } else {
            RoundInput::Prompt(&noted)
        };
        let transcript = std::mem::take(&mut self.transcript);
        let round = runner.run_round(transcript, round_input).await;
        let tracked: BTreeSet<String> = self.versions.keys().chain(&round.touched_files).cloned().collect();
        let file_versions = versions(&root, tracked).await;
        let worktree = match self.current_worktree() {
            Some(worktree) => Some(settle(worktree).await),
            None => None,
        };
        let worktree_held = self.worktree.take().map(WorktreeLease::release);
        let (record, worktree_error) = match worktree {
            Some(Ok(record)) => (Some(record), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, None),
        };
        self.finish(&round, &file_versions, record.as_ref());
        let saved = session
            .record_subagent_task(&self.task, self.new_messages(&round))
            .await;
        Outcome {
            round,
            task: self.task,
            save_error: saved.err().map(|e| e.to_string()),
            stale,
            others,
            worktree: worktree_held.filter(|_| record.as_ref().is_some_and(|r| !r.removed)),
            record,
            worktree_error,
        }
    }

    fn moved_text(&self) -> String {
        match (&self.previous, self.current_worktree()) {
            (Some(previous), Some(current)) if previous.id != current.id => moved_note(previous, current),
            _ => String::new(),
        }
    }

    /// Files that differ from what the subagent last saw, other than the ones it read.
    async fn other_changes(&self, root: &std::path::Path, stale: &[String]) -> Vec<WorkspaceChange> {
        let Some(previous) = &self.previous else {
            return Vec::new();
        };
        let fresh_base = self
            .current_worktree()
            .filter(|w| w.id != previous.id)
            .map(|w| w.base.as_str());
        changed_since(root, &previous.head, fresh_base)
            .await
            .into_iter()
            .filter(|change| !stale.contains(&change.path))
            .collect()
    }

    fn finish(
        &mut self,
        round: &Round,
        file_versions: &BTreeMap<String, Fingerprint>,
        worktree: Option<&WorktreeRecord>,
    ) {
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
            self.add_artifact(RESULT_ARTIFACT, Part::text(text.trim()));
        }
        let data = [
            (
                FILES_CHANGED_ARTIFACT,
                serde_json::json!(round.changed_files),
                round.changed_files.is_empty(),
            ),
            (
                WORKSPACE_CHANGES_ARTIFACT,
                serde_json::json!(round.workspace_changes),
                round.workspace_changes.is_empty(),
            ),
            (
                TOOL_CALLS_ARTIFACT,
                serde_json::json!(round.tool_log),
                round.tool_log.is_empty(),
            ),
        ];
        for (name, value, empty) in data {
            if !empty {
                self.add_artifact(name, Part::data(value));
            }
        }
        if !file_versions.is_empty() {
            self.add_artifact(FILE_VERSIONS_ARTIFACT, Part::data(serde_json::json!(file_versions)));
        }
        if let Some(record) = worktree {
            self.add_artifact(WORKTREE_ARTIFACT, Part::data(serde_json::json!(record)));
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
    /// Files the subagent was told had changed since it last saw them.
    pub stale: Vec<String>,
    /// Other workspace files the subagent was told had changed since its last round.
    pub others: Vec<WorkspaceChange>,
    pub worktree: Option<Worktree>,
    pub record: Option<WorktreeRecord>,
    pub worktree_error: Option<String>,
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
            Ok(body) => SubagentRunner::with_evidence(&body, round),
            Err(error) => error,
        };
        if !self.stale.is_empty() {
            text.push_str(&format!(
                "\nTold the subagent these files changed since it last saw them: {}",
                self.stale.join(", ")
            ));
        }
        if !self.others.is_empty() {
            text.push_str(&format!(
                "\nTold the subagent {} other workspace file(s) changed since its last round.",
                self.others.len()
            ));
        }
        if let Some(record) = &self.record {
            text.push_str(&pending_note(record));
        }
        if let Some(error) = &self.worktree_error {
            text.push_str(&format!(
                "\n\nWarning: the subagent's worktree could not be committed ({error}); check it with the worktree tool's list action."
            ));
        }
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
                "\n\nThe subagent is waiting. Reply with subagent {{\"task_id\": \"{id}\", \"prompt\": \"<answer or go-ahead>\"}}. Inspect its tool calls and evidence with {{\"task_id\": \"{id}\", \"inspect\": true}}."
            ),
            _ => format!(
                "\n\nVerify before accepting: review the changed files and re-run your checks. Inspect its tool calls with subagent {{\"task_id\": \"{id}\", \"inspect\": true}}; request fixes with {{\"task_id\": \"{id}\", \"prompt\": \"<feedback>\"}} (it continues in the same worktree until you merge or discard it)."
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
        assert_eq!(
            artifact_text(task, RESULT_ARTIFACT),
            Some("Latency: hoisted the allocation out of hot().")
        );
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
        assert!(
            first.contains(&format!("{{\"task_id\": \"{first_id}\", \"prompt\"")),
            "{first}"
        );
        assert!(first.contains("Verify before accepting"), "{first}");
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

    #[tokio::test]
    async fn inspecting_shows_recorded_evidence_without_running_the_subagent() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(dir.path(), None).unwrap();
        let secret = dir.path().join("app.env");
        let adapter = MockAdapter::from_turns([
            vec![
                ModelStreamEvent::ToolCall(ToolCall::new(
                    "c1",
                    ToolFunction::new("read", json!({ "path": "src/hot.rs" })),
                )),
                ModelStreamEvent::ToolCall(ToolCall::new(
                    "c2",
                    ToolFunction::new("read", json!({ "path": secret.to_string_lossy() })),
                )),
            ],
            text("hot() allocates in a loop."),
        ]);
        let scope = crate::permission::policy::parse_scope_from_str(
            "[permission.path]\n\"*.env\" = { action = \"deny\", reason = \"secrets\" }\n",
        )
        .unwrap();
        let hook = crate::permission::PermissionHook::with_policy(
            Some(dir.path().to_path_buf()),
            Arc::new(crate::permission::tests::mock::MockHookPresenter::new(false, None)),
            crate::permission::policy::build_policy(None, Some(scope)),
        );
        let ctx = TurnToolContext {
            session: Some(session.clone()),
            permission: Some(Arc::new(hook)),
            ..Default::default()
        };
        let tool = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()])
            .bind_turn(&ctx)
            .unwrap();

        let result = ask(&tool, json!({ "name": "audit", "prompt": "Audit hot.rs" })).await;
        let id = task_id(&result);
        assert!(result.contains("Tool outcomes: 1 ok, 1 denied (read "), "{result}");
        assert!(result.contains("1 failed tool call, "), "{result}");
        let requests = adapter.requests().len();

        let inspected = ask(&tool, json!({ "task_id": id, "inspect": true })).await;
        let listed = ask(&tool, json!({ "inspect": true })).await;

        assert_eq!(adapter.requests().len(), requests, "inspecting runs no model");
        for expected in [
            format!("Task `{id}` \u{b7} completed \u{b7} agent scout, name audit (task 1 of 1"),
            "[parent] Audit hot.rs".to_string(),
            "Final answer:\nhot() allocates in a loop.".to_string(),
            "ok      read src/hot.rs".to_string(),
            "denied  read ".to_string(),
            "Files read or edited so far: ".to_string(),
        ] {
            assert!(inspected.contains(&expected), "missing {expected:?} in:\n{inspected}");
        }
        let summary = format!(
            "`{id}` \u{b7} completed, 1 failed tool calls \u{b7} agent scout, name audit \u{b7} 1 task(s) \u{b7} \"Audit hot.rs\""
        );
        assert!(listed.contains(&summary), "{listed}");
        let unknown = tool
            .execute(json!({ "task_id": "nope", "inspect": true }))
            .await
            .unwrap();
        assert!(unknown.is_error && unknown.content.contains("unknown subagent task 'nope'"));
        let no_session = ask(&bound_tool(&adapter, None), json!({ "inspect": true })).await;
        assert!(no_session.contains("needs a saved session"), "{no_session}");
    }

    #[tokio::test]
    async fn a_continued_subagent_is_told_which_files_changed_since_it_read_them() {
        let dir = tempfile::tempdir().unwrap();
        let session = SessionManager::new(&dir.path().join("sessions"), None).unwrap();
        let watched = dir.path().join("lib.rs");
        let untouched = dir.path().join("other.rs");
        std::fs::write(&watched, "fn a() {}").unwrap();
        std::fs::write(&untouched, "fn b() {}").unwrap();
        let read = |path: &std::path::Path| call("read", json!({ "path": path.to_string_lossy() }));
        let adapter = MockAdapter::from_turns([read(&watched), read(&untouched), text("reviewed")]);
        let tool = bound_tool(&adapter, Some(session));
        let first = ask(&tool, json!({ "prompt": "Review lib.rs" })).await;

        std::fs::write(&watched, "fn a() { changed() }").unwrap();
        adapter.add_turn(vec![Ok(ModelStreamEvent::Text("re-reviewed".into()))]);
        let second = ask(&tool, json!({ "task_id": task_id(&first), "prompt": "Check again" })).await;

        let shown = watched.to_string_lossy().to_string();
        let prompt = adapter.requests().pop().unwrap().prompt;
        assert_eq!(
            prompt,
            format!(
                "[These files changed since you last read or edited them; re-read them before relying on what you saw earlier: {shown}]\n\nCheck again"
            )
        );
        let told = format!("Told the subagent these files changed since it last saw them: {shown}\n");
        assert!(second.contains(&told), "{second}");
    }

    #[tokio::test]
    async fn bash_changes_are_attributed_and_later_changes_reach_a_continued_subagent() {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path().canonicalize().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@e", "-c", "user.name=t"])
                .args(args)
                .current_dir(&root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("other.rs"), "v1").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "init"]);
        let sessions = tempfile::tempdir().unwrap();
        let session = SessionManager::new(sessions.path(), None).unwrap();
        let bash_root = root.clone();
        let bash = GenericTool::new("bash", "runs", json!({}), move |_| {
            let target = bash_root.join("gen.rs");
            async move {
                std::fs::write(target, "generated").unwrap();
                Ok(ToolResult::success("done"))
            }
        });
        let outside_root = root.clone();
        let outside = GenericTool::new(
            "web_fetch",
            "stands in for a change made by another process",
            json!({}),
            move |_| {
                let target = outside_root.join("notes.md");
                async move {
                    std::fs::write(target, "edited elsewhere").unwrap();
                    Ok(ToolResult::success("ok"))
                }
            },
        );
        let adapter = MockAdapter::from_turns([
            vec![
                ModelStreamEvent::ToolCall(ToolCall::new(
                    "b",
                    ToolFunction::new("bash", json!({ "command": "make gen" })),
                )),
                ModelStreamEvent::ToolCall(ToolCall::new(
                    "w",
                    ToolFunction::new("web_fetch", json!({ "url": "x" })),
                )),
            ],
            text("generated"),
        ]);
        let ctx = TurnToolContext {
            session: Some(session),
            project: Some(Arc::new(crate::engine::context::ProjectContext::for_tests(&root))),
            ..Default::default()
        };
        let tool = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![Arc::new(bash), Arc::new(outside)])
            .bind_turn(&ctx)
            .unwrap();

        let first = ask(&tool, json!({ "role": "general", "prompt": "generate" })).await;

        assert!(
            first.contains("Changed while its bash commands ran: gen.rs (bash: make gen)\n"),
            "{first}"
        );
        assert!(
            first.contains("not by its tools (editors, other agents, or processes): notes.md\n"),
            "{first}"
        );
    }

    struct WorktreeSetup {
        dir: tempfile::TempDir,
        root: std::path::PathBuf,
        store: crate::worktree::WorktreeStore,
        adapter: MockAdapter,
        tool: Arc<dyn EngineTool>,
    }

    fn write_turn(path: &str, body: &str) -> Vec<ModelStreamEvent> {
        call("write", json!({ "path": path, "content": body }))
    }

    /// A repository and a subagent tool whose `write` runs for real inside each subagent's worktree.
    fn worktree_setup(turns: Vec<Vec<ModelStreamEvent>>) -> WorktreeSetup {
        let (dir, root) = crate::worktree::git::test_repo::create(&[("a.rs", "base\n"), ("other.rs", "v1\n")]);
        let store = crate::worktree::WorktreeStore::new(dir.path().join("store"));
        let session = SessionManager::new(&dir.path().join("sessions"), None).unwrap();
        let placeholder: Arc<dyn EngineTool> = Arc::new(GenericTool::new(
            "write",
            "replaced in the worktree",
            json!({}),
            |_| async { Ok(ToolResult::error("the placeholder must never run")) },
        ));
        let adapter = MockAdapter::from_turns(turns);
        let ctx = TurnToolContext {
            session: Some(session),
            project: Some(Arc::new(crate::engine::context::ProjectContext::for_tests(&root))),
            ..Default::default()
        };
        let tool = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![placeholder])
            .with_worktrees(store.clone())
            .bind_turn(&ctx)
            .unwrap();
        WorktreeSetup {
            dir,
            root,
            store,
            adapter,
            tool,
        }
    }

    #[tokio::test]
    async fn subagents_work_in_their_own_worktree_and_feedback_continues_there() {
        let s = worktree_setup(vec![write_turn("a.rs", "agent\n"), text("wrote a.rs")]);

        let first = ask(
            &s.tool,
            json!({ "name": "fixer", "role": "general", "prompt": "fix a.rs" }),
        )
        .await;
        let worktree = s.store.list(&s.root).await.pop().unwrap();

        assert!(
            first.contains(&format!("These changes are in worktree `{}`", worktree.id)),
            "{first}"
        );
        assert_eq!(
            std::fs::read_to_string(s.root.join("a.rs")).unwrap(),
            "base\n",
            "main is untouched"
        );
        assert_eq!(std::fs::read_to_string(worktree.path.join("a.rs")).unwrap(), "agent\n");

        s.adapter
            .add_turn(write_turn("b.rs", "more\n").into_iter().map(Ok).collect());
        s.adapter
            .add_turn(vec![Ok(ModelStreamEvent::Text("added b.rs".into()))]);
        let second = ask(
            &s.tool,
            json!({ "task_id": task_id(&first), "prompt": "also add b.rs" }),
        )
        .await;

        assert_eq!(
            s.store.list(&s.root).await,
            std::slice::from_ref(&worktree),
            "feedback continues in the same worktree"
        );
        assert!(
            second.contains("not in your working tree: a.rs, b.rs (added)."),
            "{second}"
        );
        assert!(second.contains("Files changed: b.rs\nTool outcomes"), "{second}");
        assert!(!s.adapter.requests().pop().unwrap().prompt.contains("previous worktree"));
    }

    #[tokio::test]
    async fn after_a_merge_a_continued_subagent_gets_a_fresh_worktree_and_hears_what_changed() {
        let s = worktree_setup(vec![write_turn("a.rs", "agent\n"), text("wrote a.rs")]);
        let first = ask(&s.tool, json!({ "role": "general", "prompt": "fix a.rs" })).await;
        let worktree = s.store.list(&s.root).await.pop().unwrap();
        std::fs::write(s.root.join("other.rs"), "parent edit\n").unwrap();
        let parent = crate::tools::WorktreeTool::new(&s.root, s.store.clone());

        let merged = parent
            .execute(json!({ "action": "merge", "id": worktree.id }))
            .await
            .unwrap();
        s.adapter
            .add_turn(vec![Ok(ModelStreamEvent::Text("looked again".into()))]);
        let again = ask(
            &s.tool,
            json!({ "task_id": task_id(&first), "prompt": "check other.rs" }),
        )
        .await;

        assert!(
            merged.content.starts_with("Merged 1 file(s): a.rs."),
            "{}",
            merged.content
        );
        assert_eq!(std::fs::read_to_string(s.root.join("a.rs")).unwrap(), "agent\n");
        let prompt = s.adapter.requests().pop().unwrap().prompt;
        assert!(prompt.starts_with("[Your previous worktree "), "{prompt}");
        assert!(
            prompt
                .ends_with("[Other files changed in the workspace since your last turn: other.rs.]\n\ncheck other.rs"),
            "{prompt}"
        );
        assert!(
            !again.contains("These changes are in worktree"),
            "no changes, no worktree: {again}"
        );
        assert!(s.store.list(&s.root).await.is_empty());
        drop(s.dir);
    }

    #[test]
    fn only_subagents_limited_to_read_only_tools_run_in_parallel() {
        let adapter = MockAdapter::from_turns([]);
        let write: Arc<dyn EngineTool> = Arc::new(GenericTool::new("write", "writes", json!({}), |_| async {
            Ok(ToolResult::success("written"))
        }));
        let tool = SubagentTool::new(Some(Arc::new(adapter)), vec![read_tool(), write]);
        let cases = [
            (json!({ "role": "scout", "prompt": "x" }), true),
            (json!({ "role": "general", "prompt": "x" }), false),
            (json!({ "task_id": "t-1", "prompt": "x" }), false),
            (json!({ "inspect": true }), true),
        ];
        for (args, parallel) in cases {
            assert_eq!(tool.concurrency_safe(&args), parallel, "{args}");
        }
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
