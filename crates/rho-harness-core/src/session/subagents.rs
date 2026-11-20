use super::SessionManager;
use super::format::{SessionRecord, append_durable_record};
use crate::a2a::Task;
use crate::error::Result;
use crate::model::ChatMessage;
use chrono::Utc;
use std::collections::BTreeMap;

/// One subagent conversation (an A2A context): its tasks in creation order and the subagent's own messages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubagentContext {
    pub tasks: Vec<Task>,
    pub transcript: Vec<ChatMessage>,
}

impl SubagentContext {
    #[must_use]
    pub fn latest_task(&self) -> Option<&Task> {
        self.tasks.last()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubagentContexts {
    contexts: BTreeMap<String, SubagentContext>,
}

impl SubagentContexts {
    /// Stores the task snapshot, replacing an earlier snapshot of the same task, and extends the
    /// transcript, or replaces it when the conversation was compacted.
    pub(crate) fn record(&mut self, task: Task, transcript: Vec<ChatMessage>, replaces_transcript: bool) {
        let context = self.contexts.entry(task.context_id.clone()).or_default();
        if replaces_transcript {
            context.transcript = transcript;
        } else {
            context.transcript.extend(transcript);
        }
        match context.tasks.iter_mut().find(|t| t.id == task.id) {
            Some(existing) => *existing = task,
            None => context.tasks.push(task),
        }
    }

    fn context_of_task(&self, task_id: &str) -> Option<&SubagentContext> {
        self.contexts
            .values()
            .find(|context| context.tasks.iter().any(|t| t.id == task_id))
    }

    fn all(&self) -> Vec<SubagentContext> {
        self.contexts.values().cloned().collect()
    }
}

impl SessionManager {
    /// Persists a subagent task snapshot with the transcript messages added since the context's last record.
    pub async fn record_subagent_task(&self, task: &Task, transcript: Vec<ChatMessage>) -> Result<()> {
        self.save_subagent_task(task, transcript, false).await
    }

    /// Persists a subagent task snapshot with the context's whole transcript, replacing the saved one
    /// (the conversation was compacted, so it no longer extends what was saved).
    pub async fn replace_subagent_transcript(&self, task: &Task, transcript: Vec<ChatMessage>) -> Result<()> {
        self.save_subagent_task(task, transcript, true).await
    }

    async fn save_subagent_task(&self, task: &Task, transcript: Vec<ChatMessage>, replaces: bool) -> Result<()> {
        self.reject_secrets(&(task, &transcript))?;
        let mut state = self.state.lock().await;
        let record = SessionRecord::SubagentTask {
            sequence: state.next_sequence,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            task: task.clone(),
            transcript: transcript.clone(),
            replaces_transcript: replaces,
        };
        append_durable_record(&self.file_path, &record).await?;
        state.next_sequence += 1;
        state.subagents.record(task.clone(), transcript, replaces);
        Ok(())
    }

    /// The subagent conversation that contains `task_id`.
    pub async fn subagent_context(&self, task_id: &str) -> Option<SubagentContext> {
        self.state.lock().await.subagents.context_of_task(task_id).cloned()
    }

    pub async fn subagent_contexts(&self) -> Vec<SubagentContext> {
        self.state.lock().await.subagents.all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::TaskState;

    fn task(id: &str, context: &str, state: TaskState) -> Task {
        let mut task = Task::new(id, context, None);
        task.set_status(state, None);
        task
    }

    #[tokio::test]
    async fn subagent_tasks_and_transcripts_survive_reopening_the_session() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new(&dir, None).unwrap();
        let first = vec![ChatMessage::user("audit"), ChatMessage::assistant("found a hot loop")];
        session
            .record_subagent_task(&task("t1", "c1", TaskState::Working), first.clone())
            .await
            .unwrap();
        session
            .record_subagent_task(&task("t1", "c1", TaskState::Completed), Vec::new())
            .await
            .unwrap();
        let second = vec![ChatMessage::user("also check alloc"), ChatMessage::assistant("done")];
        session
            .record_subagent_task(&task("t2", "c1", TaskState::InputRequired), second.clone())
            .await
            .unwrap();
        session
            .record_subagent_task(&task("other", "c2", TaskState::Completed), Vec::new())
            .await
            .unwrap();
        let id = session.session_id.clone();
        drop(session);

        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        let context = reopened.subagent_context("t1").await.unwrap();

        assert_eq!(context.tasks.len(), 2);
        assert_eq!(context.tasks[0].status.state, TaskState::Completed);
        assert_eq!(context.latest_task().unwrap().id, "t2");
        assert_eq!(context.transcript, [first, second].concat());
        assert_eq!(reopened.subagent_context("t2").await, Some(context));
        assert!(reopened.subagent_context("missing").await.is_none());
        assert_eq!(reopened.subagent_contexts().await.len(), 2);
    }

    #[tokio::test]
    async fn a_compacted_transcript_replaces_the_saved_one_and_survives_reopening() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new(&dir, None).unwrap();
        let long = vec![
            ChatMessage::user("audit"),
            ChatMessage::assistant("step"),
            ChatMessage::assistant("step"),
        ];
        session
            .record_subagent_task(&task("t1", "c1", TaskState::Completed), long)
            .await
            .unwrap();
        let compacted = vec![
            ChatMessage::System {
                content: "summary".into(),
            },
            ChatMessage::assistant("done"),
        ];
        session
            .replace_subagent_transcript(&task("t2", "c1", TaskState::Completed), compacted.clone())
            .await
            .unwrap();
        session
            .record_subagent_task(&task("t3", "c1", TaskState::Completed), vec![ChatMessage::user("more")])
            .await
            .unwrap();
        let id = session.session_id.clone();
        drop(session);

        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        let context = reopened.subagent_context("t1").await.unwrap();

        assert_eq!(
            context.transcript,
            [compacted, vec![ChatMessage::user("more")]].concat()
        );
        assert_eq!(context.tasks.len(), 3);
    }
}
