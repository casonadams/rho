use super::handoff::{WORKTREE_ARTIFACT, WorktreeRecord, changed_since, moved_note, pending_note, settle};
use super::round::{Round, RoundEnd, RoundInput};
use super::runner::SubagentRunner;
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
pub(crate) const RESULT_DATA_ARTIFACT: &str = "result-data";
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
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
        settings_metadata(settings),
    )
}

fn settings_metadata(settings: &ConversationSettings) -> Option<serde_json::Value> {
    serde_json::to_value(settings)
        .inspect_err(|error| tracing::warn!("saving subagent settings failed: {error}"))
        .ok()
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
    /// Keeps the conversation marked busy until the round ends.
    claim: Option<BusyClaim>,
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
            claim: None,
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
            claim: None,
        })
    }

    /// Marks the conversation busy for this round, refusing a second message while it runs.
    pub(crate) fn claimed(mut self, busy: &BusyContexts) -> Result<Self, String> {
        self.claim = Some(busy.claim(self.context_id())?);
        Ok(self)
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

    /// Records `settings` on the task, so later continuations run the same way.
    #[must_use]
    pub(crate) fn with_settings(mut self, settings: &ConversationSettings) -> Self {
        self.task.metadata = settings_metadata(settings);
        self
    }

    pub(crate) fn context_id(&self) -> &str {
        &self.task.context_id
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.task.id
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
        }
        .with_task_id(&self.task.id);
        let lsp_guard = runner.worktree_lsp_guard();
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
        lsp_guard.close_unless_cancelled(round.end == RoundEnd::Cancelled).await;
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
        let saved = if round.compacted {
            session
                .replace_subagent_transcript(&self.task, self.kept_messages(&round).to_vec())
                .await
        } else {
            session
                .record_subagent_task(&self.task, self.new_messages(&round))
                .await
        };
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
        if let Some(data) = &round.data {
            self.add_artifact(RESULT_DATA_ARTIFACT, Part::data(data.clone()));
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

    /// The round's transcript without tool calls left unanswered by an interruption.
    fn kept_messages<'a>(&self, round: &'a Round) -> &'a [ChatMessage] {
        let kept = match round.end {
            RoundEnd::Asked(_) => round.transcript.len(),
            _ => without_unanswered_tool_calls(&round.transcript).len(),
        };
        &round.transcript[..kept]
    }

    /// The transcript messages this round added, without tool calls left unanswered by an interruption.
    fn new_messages(&self, round: &Round) -> Vec<ChatMessage> {
        let kept = self.kept_messages(round);
        kept[self.saved_len.min(kept.len())..].to_vec()
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
#[path = "conversation_tests.rs"]
mod tests;
