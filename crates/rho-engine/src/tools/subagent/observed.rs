//! Tools a subagent's turn runs through: each call is recorded as evidence for its result (outcome,
//! files read and changed, what changed while a bash command ran), and `ask_parent` puts a question
//! to the delegating agent.

use super::evidence::{OutcomeStatus, ToolOutcome};
use super::snapshot::Snapshot;
use super::workspace::{WorkspaceChange, bash_file_arguments, normalize};
use crate::engine::runner::turn::runner::ToolObserver;
use crate::engine::runner::turn::types::{CancellationSignal, unless_cancelled};
use crate::locking::lock;
use crate::mailbox::{Envelope, Mailbox, Party};
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::types::ToolResult;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ToolCall;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

const MAX_TOOL_OUTPUT_CHARS: usize = 20_000;
const FILE_CHANGING_TOOLS: [&str; 2] = ["write", "edit"];
const FILE_READING_TOOLS: [&str; 3] = ["read", "write", "edit"];
pub(crate) const ASK_PARENT_TOOL: &str = "ask_parent";
const ASK_PARENT_DESCRIPTION: &str = "Ask the agent that delegated this task a question. Use it only when you cannot proceed without a decision or information you cannot find yourself. The reply is this call's result; if the delegating agent cannot answer now, your turn pauses with the question and the reply arrives as the next message.";
const QUESTION_SENT: &str = "Question sent to the delegating agent; its reply arrives as the next message.";
const NO_QUESTION: &str = "(the subagent asked without a question)";

pub(crate) fn truncate_output(output: String) -> String {
    let total = output.chars().count();
    if total <= MAX_TOOL_OUTPUT_CHARS {
        return output;
    }
    let kept: String = output.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    format!("{kept}\n[truncated {} characters]", total - MAX_TOOL_OUTPUT_CHARS)
}

/// What the subagent's tool calls did, gathered as they run.
#[derive(Default)]
pub(crate) struct Evidence {
    pub changed_files: BTreeSet<String>,
    pub touched_files: BTreeSet<String>,
    pub tool_log: Vec<ToolOutcome>,
    /// Path -> the bash command that was running when the path changed.
    pub bash_changes: BTreeMap<String, String>,
}

/// Collects evidence for one round: the wrapped tools report what bash commands changed or read,
/// and the turn loop reports how each call ended, after outcome hooks.
pub(crate) struct Recorder {
    evidence: Mutex<Evidence>,
    /// Bash effects waiting for their call's outcome, by tool name and arguments, oldest first.
    pending: Mutex<BTreeMap<String, VecDeque<BashEffects>>>,
    root: PathBuf,
    /// Snapshots run only in real sessions, which have a project directory.
    snapshots: bool,
}

fn call_key(name: &str, args: &Value) -> String {
    format!("{name} {args}")
}

impl Recorder {
    pub(crate) fn new(root: PathBuf, snapshots: bool) -> Arc<Self> {
        Arc::new(Self {
            evidence: Mutex::default(),
            pending: Mutex::default(),
            root,
            snapshots,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Evidence> {
        lock(&self.evidence)
    }

    pub(crate) fn take(&self) -> Evidence {
        std::mem::take(&mut *self.lock())
    }

    fn hold(&self, name: &str, args: &Value, bash: BashEffects) {
        lock(&self.pending)
            .entry(call_key(name, args))
            .or_default()
            .push_back(bash);
    }

    fn release(&self, name: &str, args: &Value) -> BashEffects {
        lock(&self.pending)
            .get_mut(&call_key(name, args))
            .and_then(VecDeque::pop_front)
            .unwrap_or_default()
    }

    fn record(&self, name: &str, args: &Value, output: &str, is_error: bool) {
        let bash = self.release(name, args);
        let status = if is_error {
            OutcomeStatus::Error
        } else {
            OutcomeStatus::Ok
        };
        let mut logged = ToolOutcome::new(name, args, status, output);
        let path = args["path"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .map(|p| normalize(&self.root, p));
        if let Some(path) = &path {
            logged.target.clone_from(path);
        }
        let mut evidence = self.lock();
        evidence.tool_log.push(logged);
        let command = args["command"].as_str().map(|c| super::evidence::clip(c, 60));
        for changed in bash.changes {
            evidence
                .bash_changes
                .insert(changed, command.clone().unwrap_or_default());
        }
        evidence.touched_files.extend(bash.reads);
        let Some(path) = path else {
            return;
        };
        if FILE_READING_TOOLS.contains(&name) {
            evidence.touched_files.insert(path.clone());
        }
        if !is_error && FILE_CHANGING_TOOLS.contains(&name) {
            evidence.changed_files.insert(path);
        }
    }
}

impl ToolObserver for Recorder {
    fn denied(&self, call: &ToolCall, reason: &str) {
        self.lock().tool_log.push(ToolOutcome::new(
            &call.function.name,
            &call.function.arguments,
            OutcomeStatus::Denied,
            reason,
        ));
    }

    fn finished(&self, call: &ToolCall, args: &Value, output: &str, is_error: bool) {
        self.record(&call.function.name, args, output, is_error);
    }
}

impl Evidence {
    /// The changes found by the snapshots around the subagent's bash calls.
    pub(crate) fn bash_workspace_changes(&self, root: &std::path::Path) -> BTreeSet<WorkspaceChange> {
        self.bash_changes
            .iter()
            .map(|(path, command)| WorkspaceChange {
                path: path.clone(),
                deleted: !root.join(path).exists(),
                by: Some(command.clone()),
            })
            .collect()
    }
}

#[derive(Default)]
struct BashEffects {
    changes: Vec<String>,
    reads: Vec<String>,
}

/// A tool whose every call is recorded, with its output clipped to what a subagent passes on.
pub(crate) struct RecordedTool {
    inner: Arc<dyn EngineTool>,
    recorder: Arc<Recorder>,
}

impl RecordedTool {
    pub(crate) fn wrap(inner: Arc<dyn EngineTool>, recorder: &Arc<Recorder>) -> Arc<dyn EngineTool> {
        Arc::new(Self {
            inner,
            recorder: recorder.clone(),
        })
    }

    async fn snapshot(&self, previous: Option<&Snapshot>) -> Option<Snapshot> {
        if !self.recorder.snapshots {
            return None;
        }
        match Snapshot::take(&self.recorder.root, previous).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!("subagent bash snapshot failed: {error}");
                None
            }
        }
    }
}

#[async_trait::async_trait]
impl EngineTool for RecordedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn concurrency_safe(&self, args: &Value) -> bool {
        self.inner.concurrency_safe(args)
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        let name = self.inner.name();
        let bash_command = (name == "bash").then(|| args["command"].as_str().unwrap_or_default().to_string());
        let before = match bash_command {
            Some(_) => self.snapshot(None).await,
            None => None,
        };
        let mut result = match self.inner.execute(args.clone()).await {
            Ok(result) => result,
            Err(e) => ToolResult::error(format!("Error: {e}")),
        };
        let mut bash = BashEffects::default();
        if let Some(before) = &before
            && let Some(after) = self.snapshot(Some(before)).await
        {
            bash.changes = after
                .changes_since(before, &self.recorder.root)
                .files
                .into_iter()
                .map(|change| change.path)
                .collect();
        }
        if let Some(command) = &bash_command {
            bash.reads = bash_file_arguments(&self.recorder.root, command).await;
        }
        result.content = truncate_output(result.content);
        result.display = None;
        self.recorder.hold(name, &args, bash);
        Ok(result)
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        let bound = self.inner.bind_turn(ctx).unwrap_or_else(|| self.inner.clone());
        Some(Self::wrap(bound, &self.recorder))
    }
}

/// Lets the subagent ask the delegating agent a question. With a live mailbox the question goes to
/// the parent and the call returns its reply, so the round keeps its work in progress; when the
/// parent declines (it waits or ends its turn instead), the call ends the turn and the question is
/// kept for the round's result, to be answered by continuing the task.
pub(crate) struct AskParentTool {
    questions: Arc<Mutex<Vec<String>>>,
    inbox: Option<Mailbox>,
    cancellation: Option<CancellationSignal>,
}

impl AskParentTool {
    /// The tool, and where it keeps the questions the subagent asks.
    pub(crate) fn with_question_slot() -> (Arc<dyn EngineTool>, Arc<Mutex<Vec<String>>>) {
        let questions = Arc::new(Mutex::new(Vec::new()));
        let tool = Arc::new(Self {
            questions: questions.clone(),
            inbox: None,
            cancellation: None,
        });
        (tool, questions)
    }

    /// Ends the turn with `question` kept for the round's result.
    fn pause(&self, question: String) -> ToolResult {
        lock(&self.questions).push(question);
        ToolResult {
            ends_turn: true,
            ..ToolResult::success(QUESTION_SENT)
        }
    }

    /// The parent's reply to `question`, or `None` when the question could not be put, the parent
    /// declined, or the round is stopping.
    async fn ask(&self, inbox: &Mailbox, question: &str) -> Option<Envelope> {
        let request = match inbox.ask_parent(&reply_hint(inbox.id().as_str(), question)) {
            Ok(request) => request,
            Err(error) => {
                tracing::warn!(
                    "subagent {} could not ask its parent, pausing instead: {error}",
                    inbox.id()
                );
                return None;
            }
        };
        let reply = unless_cancelled(self.cancellation.as_ref(), inbox.reply(&request)).await;
        let Some(Some(reply)) = reply else {
            inbox.withdraw(&request);
            return None;
        };
        (reply.from != Party::System).then_some(reply)
    }
}

/// The question as the parent receives it, with how to answer.
fn reply_hint(task_id: &str, question: &str) -> String {
    let question = if question.is_empty() { NO_QUESTION } else { question };
    format!(
        "{question}\n\nAnswer with subagent {{\"task_id\": \"{task_id}\", \"prompt\": \"<answer>\"}}; it waits for your reply with its work in progress. If you wait or end your turn instead, it pauses with this question, and you can answer it the same way later."
    )
}

/// The questions asked this turn as one: blank asks only count when nothing real was asked.
pub(crate) fn take_question(slot: &Mutex<Vec<String>>) -> Option<String> {
    let asked = std::mem::take(&mut *lock(slot));
    if asked.is_empty() {
        return None;
    }
    let real: Vec<String> = asked.into_iter().filter(|q| !q.is_empty()).collect();
    Some(if real.is_empty() {
        NO_QUESTION.to_string()
    } else {
        real.join("\n")
    })
}

#[async_trait::async_trait]
impl EngineTool for AskParentTool {
    fn name(&self) -> &str {
        ASK_PARENT_TOOL
    }

    fn description(&self) -> &str {
        ASK_PARENT_DESCRIPTION
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": { "question": { "type": "string", "description": "The specific question to ask" } },
            "required": ["question"]
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        let asked = args["question"].as_str().unwrap_or_default().trim().to_string();
        let Some(inbox) = &self.inbox else {
            return Ok(self.pause(asked));
        };
        Ok(match self.ask(inbox, &asked).await {
            Some(reply) => ToolResult::success(inbox.for_model(&[reply]).concat()),
            None => self.pause(asked),
        })
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(Self {
            questions: self.questions.clone(),
            inbox: ctx.inbox.clone(),
            cancellation: ctx.cancellation.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(&'static str);

    #[async_trait::async_trait]
    impl EngineTool for Echo {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn parameters(&self) -> Value {
            serde_json::json!({})
        }
        async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
            match args["fail"].as_bool() {
                Some(true) => Err(AppError::Other(anyhow::anyhow!("broke"))),
                _ => Ok(ToolResult::success("x".repeat(MAX_TOOL_OUTPUT_CHARS + 3))),
            }
        }
    }

    fn call(name: &str, args: &Value) -> ToolCall {
        ToolCall::new("c", rho_harness_core::model::ToolFunction::new(name, args.clone()))
    }

    /// Runs `tool` and reports its outcome to `recorder` the way the turn loop does, after a hook
    /// that rewrites `secret` in the output.
    async fn run(recorder: &Recorder, tool: &Arc<dyn EngineTool>, args: Value) -> ToolResult {
        let result = tool.execute(args.clone()).await.unwrap();
        let hooked = result.content.replace("secret", "[REDACTED]");
        recorder.finished(&call(tool.name(), &args), &args, &hooked, result.is_error);
        result
    }

    #[tokio::test]
    async fn every_call_is_recorded_with_its_final_output_and_the_files_it_read_or_changed() {
        let recorder = Recorder::new(PathBuf::from("/repo"), false);
        let edit = RecordedTool::wrap(Arc::new(Echo("edit")), &recorder);
        let read = RecordedTool::wrap(Arc::new(Echo("read")), &recorder);

        let edited = run(&recorder, &edit, serde_json::json!({ "path": "src/a.rs" })).await;
        run(&recorder, &read, serde_json::json!({ "path": "/repo/src/b.rs" })).await;
        let failed = run(
            &recorder,
            &edit,
            serde_json::json!({ "path": "src/c.rs", "fail": true }),
        )
        .await;
        recorder.denied(&call("bash", &serde_json::json!({})), "Permission denied: no");
        let secretive = Recorder::new(PathBuf::from("/repo"), false);
        secretive.finished(
            &call("read", &serde_json::json!({})),
            &serde_json::json!({}),
            "failed: [REDACTED]",
            true,
        );

        assert!(edited.content.ends_with("[truncated 3 characters]"));
        assert!(failed.is_error && failed.content.starts_with("Error: ") && failed.content.contains("broke"));
        let evidence = recorder.take();
        assert_eq!(evidence.changed_files, BTreeSet::from(["src/a.rs".to_string()]));
        assert_eq!(
            evidence.touched_files,
            BTreeSet::from(["src/a.rs".into(), "src/b.rs".into(), "src/c.rs".into()])
        );
        let statuses: Vec<OutcomeStatus> = evidence.tool_log.iter().map(|o| o.status).collect();
        assert_eq!(
            statuses,
            [
                OutcomeStatus::Ok,
                OutcomeStatus::Ok,
                OutcomeStatus::Error,
                OutcomeStatus::Denied
            ]
        );
        let logged = serde_json::to_string(&secretive.take().tool_log).unwrap();
        assert!(
            logged.contains("[REDACTED]"),
            "evidence keeps the output after hooks: {logged}"
        );
    }

    #[tokio::test]
    async fn asking_the_parent_keeps_every_question_and_ends_the_turn() {
        let (tool, questions) = AskParentTool::with_question_slot();
        let first = tool
            .execute(serde_json::json!({ "question": " Which module? " }))
            .await
            .unwrap();
        tool.execute(serde_json::json!({})).await.unwrap();
        tool.execute(serde_json::json!({ "question": "Latency or memory?" }))
            .await
            .unwrap();

        assert!(first.ends_turn);
        assert_eq!(first.content, QUESTION_SENT);
        assert_eq!(
            take_question(&questions).as_deref(),
            Some("Which module?\nLatency or memory?"),
            "blank asks are dropped when real ones exist"
        );
        assert_eq!(take_question(&questions), None, "taken once");

        let (blank, only_blank) = AskParentTool::with_question_slot();
        blank.execute(serde_json::json!({})).await.unwrap();
        assert_eq!(take_question(&only_blank).as_deref(), Some(NO_QUESTION));
    }

    #[tokio::test]
    async fn with_a_mailbox_the_parents_reply_is_the_result_and_a_decline_pauses_the_turn() {
        let boxes = crate::mailbox::Mailboxes::default();
        let parent = boxes.open(&"boss".into(), None);
        let inbox = boxes.open(&"task".into(), Some(&"boss".into()));
        let (tool, questions) = AskParentTool::with_question_slot();
        let tool = tool
            .bind_turn(&TurnToolContext {
                inbox: Some(inbox),
                ..TurnToolContext::default()
            })
            .unwrap();
        let question = serde_json::json!({ "question": "Which module?" });

        let answering = tokio::spawn({
            let (boxes, parent) = (boxes.clone(), parent.clone());
            async move {
                parent.wait().await;
                let asked = parent.take();
                boxes
                    .tell(Party::Agent("boss".into()), &"task".into(), "The parser.")
                    .unwrap();
                asked
            }
        });
        let answered = tool.execute(question.clone()).await.unwrap();
        let asked = answering.await.unwrap();
        let declining = tokio::spawn(async move {
            parent.wait().await;
            let _ = parent.take();
            parent.decline_unanswered();
        });
        let declined = tool.execute(question).await.unwrap();
        declining.await.unwrap();

        assert!(
            asked[0]
                .body
                .starts_with("Which module?\n\nAnswer with subagent {\"task_id\": \"task\"")
        );
        assert!(!answered.ends_turn);
        assert_eq!(answered.content, "Reply from the delegating agent:\nThe parser.");
        assert!(declined.ends_turn);
        assert_eq!(declined.content, QUESTION_SENT);
        assert_eq!(take_question(&questions).as_deref(), Some("Which module?"));
    }
}
