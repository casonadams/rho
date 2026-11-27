//! The supervisor every subagent tool of a session shares, and that survives rebuilding those tools
//! (a model or login change): the agents' mailboxes, the rounds running now and how to message or
//! stop them, the conversations a round is busy with, and the limited slots rounds run in.

use super::conversation::ConversationSettings;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::locking::lock;
use crate::mailbox::{AgentId, Delivery, Mailbox, Mailboxes, Party, SendError};
use crate::tools::jobs::{JobCompletion, JobId, JobManager};
use rho_harness_core::a2ui::{SemanticTone, SubagentSurfaceRow, subagent_surface_messages};
use rho_harness_core::presentation::Presenter;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Subagent rounds that may run at once, in the foreground or the background.
pub(super) const MAX_ACTIVE_ROUNDS: usize = 8;

#[derive(Clone, Default)]
pub struct SubagentRuntime {
    mailboxes: Mailboxes,
    /// The top-level agent's mailbox, kept open so notices for it wait while no turn runs.
    root: Arc<Mutex<Option<Mailbox>>>,
    open: OpenRounds,
    busy: BusyContexts,
    rounds: Arc<Rounds>,
}

impl SubagentRuntime {
    /// The mailbox of the top-level agent `owner` (its session id), opened on first use and kept.
    pub fn inbox(&self, owner: &AgentId) -> Mailbox {
        let mut root = lock(&self.root);
        if let Some(inbox) = root.as_ref().filter(|inbox| inbox.id() == owner) {
            return inbox.clone();
        }
        let inbox = self.mailboxes.open(owner, None);
        *root = Some(inbox.clone());
        inbox
    }

    /// Sends a message the user typed to task `task_id`'s running round, answering its question if it
    /// is waiting on one.
    pub fn message_from_user(&self, task_id: &AgentId, text: &str) -> Result<Delivery, SendError> {
        self.mailboxes.tell(Party::User, task_id, text)
    }

    /// Asks task `task_id`'s running round to stop and save what it has; `false` if none is running.
    pub fn stop(&self, task_id: &AgentId) -> bool {
        self.open.with(task_id, |round| round.stop.cancel()).is_some()
    }

    /// Sends `message` from the delegating agent to task `task_id`'s running round, answering its
    /// question if it is waiting on one, and returns the confirmation; [`SendError::NoMailbox`] when
    /// no round of it runs.
    pub(crate) fn steer(&self, task_id: &AgentId, message: &str) -> Result<String, SendError> {
        let parent = self
            .open
            .with(task_id, |round| round.owner.clone())
            .ok_or(SendError::NoMailbox)?;
        Ok(match self.mailboxes.tell(Party::Agent(parent), task_id, message)? {
            Delivery::Reply => {
                format!("Reply delivered to subagent task '{task_id}'; it continues from its question.")
            }
            Delivery::Queued => {
                format!("Message queued for running subagent task '{task_id}'; it reads it before its next model call.")
            }
        })
    }

    /// What task `task_id` runs as while one of its rounds is running.
    pub(crate) fn running_settings(&self, task_id: &AgentId) -> Option<ConversationSettings> {
        self.open.with(task_id, |round| round.settings.clone())
    }

    pub(crate) fn busy(&self) -> &BusyContexts {
        &self.busy
    }

    /// A slot for a new round; [`RoundRefused::AllSlotsTaken`] when all are taken.
    pub(crate) fn reserve(&self) -> Result<RoundSlot, RoundRefused> {
        self.rounds.try_take().ok_or(RoundRefused::AllSlotsTaken)
    }

    /// Waits until a round slot is free; workflow steps queue here instead of failing.
    pub(crate) async fn wait_for_slot(&self) -> RoundSlot {
        crate::wake::until(&self.rounds.freed, || self.rounds.try_take()).await
    }

    /// Registers task `task_id`'s round, delegated by the top-level agent `owner`, and opens its
    /// mailbox under `owner`'s (opened too if it is not yet), which decides who the round may message.
    /// The round is forgotten when the returned registration closes or drops.
    pub(crate) fn open_round(
        &self,
        task_id: &AgentId,
        owner: &AgentId,
        settings: ConversationSettings,
        presenter: Option<Arc<dyn Presenter>>,
        stop: CancellationSignal,
    ) -> RunningRound {
        self.inbox(owner);
        let mailbox = self.mailboxes.open(task_id, Some(owner));
        self.open.insert(
            task_id,
            OpenRound {
                mailbox: mailbox.clone(),
                owner: owner.clone(),
                settings,
                stop,
                started_at: Instant::now(),
                presenter,
            },
        );
        RunningRound {
            open: self.open.clone(),
            task_id: task_id.clone(),
            mailbox,
            closed: false,
        }
    }

    /// The background work of this engine's agents.
    pub(crate) fn jobs(&self) -> &JobManager {
        self.mailboxes.jobs()
    }

    /// Starts tracking background work agent `owner` owns; its notice goes to `owner`.
    pub(crate) fn start_job(&self, owner: &AgentId, id: JobId, label: String) -> JobCompletion {
        self.mailboxes.start_job(owner, id, label)
    }

    #[cfg(test)]
    pub(crate) fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rounds, &other.rounds)
    }

    #[cfg(test)]
    pub(super) fn active_rounds(&self) -> usize {
        self.rounds.active.load(Ordering::Acquire)
    }
}

/// A running round: its mailbox, who delegated it, what it runs as, and the signal that stops it.
struct OpenRound {
    mailbox: Mailbox,
    owner: AgentId,
    settings: ConversationSettings,
    stop: CancellationSignal,
    started_at: Instant,
    presenter: Option<Arc<dyn Presenter>>,
}

/// The running rounds by task id.
#[derive(Clone, Default)]
struct OpenRounds(Arc<Mutex<HashMap<AgentId, OpenRound>>>);

impl OpenRounds {
    fn with<T>(&self, task_id: &AgentId, read: impl FnOnce(&OpenRound) -> T) -> Option<T> {
        lock(&self.0).get(task_id).map(read)
    }

    fn insert(&self, task_id: &AgentId, round: OpenRound) {
        let presenter = round.presenter.clone();
        lock(&self.0).insert(task_id.clone(), round);
        self.emit_surface(presenter.as_deref());
    }

    fn snapshot_rows(&self) -> Vec<SubagentSurfaceRow> {
        let mut rows: Vec<SubagentSurfaceRow> = lock(&self.0)
            .iter()
            .map(|(task_id, round)| SubagentSurfaceRow {
                id: task_id.to_string(),
                name: round.settings.name.clone().unwrap_or_else(|| task_id.to_string()),
                role: Some(round.settings.agent_or_role().to_string()),
                status: "running".to_string(),
                tone: SemanticTone::Accent,
                elapsed_ms: Some(round.started_at.elapsed().as_millis() as u64),
            })
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    fn emit_surface(&self, presenter: Option<&dyn Presenter>) {
        let Some(presenter) = presenter else {
            return;
        };
        for msg in subagent_surface_messages(&self.snapshot_rows()) {
            presenter.emit_a2ui(msg);
        }
    }

    /// Forgets `mailbox`'s round if it is still the task's open one, closes the mailbox, and returns
    /// the messages the round never read.
    fn close(&self, task_id: &AgentId, mailbox: Mailbox) -> Vec<String> {
        let mut rounds = lock(&self.0);
        let presenter = if rounds.get(task_id).is_some_and(|round| round.mailbox.same(&mailbox)) {
            rounds.remove(task_id).and_then(|round| round.presenter)
        } else {
            None
        };
        drop(rounds);
        self.emit_surface(presenter.as_deref());
        mailbox.close().into_iter().map(|m| m.body).collect()
    }
}

/// A round's registration with the runtime; closing or dropping it (the round ended or was aborted)
/// forgets the round and closes its mailbox.
pub(crate) struct RunningRound {
    open: OpenRounds,
    task_id: AgentId,
    mailbox: Mailbox,
    closed: bool,
}

impl RunningRound {
    pub(crate) fn mailbox(&self) -> Mailbox {
        self.mailbox.clone()
    }

    /// Ends the registration, returning the messages the round never read.
    pub(crate) fn close(&mut self) -> Vec<String> {
        if std::mem::replace(&mut self.closed, true) {
            return Vec::new();
        }
        self.open.close(&self.task_id, self.mailbox.clone())
    }
}

impl Drop for RunningRound {
    fn drop(&mut self) {
        self.close();
    }
}

/// Why a new subagent round cannot start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoundRefused {
    /// [`MAX_ACTIVE_ROUNDS`] rounds are already running.
    AllSlotsTaken,
    /// The conversation already has a round in progress.
    Busy,
}

impl std::fmt::Display for RoundRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllSlotsTaken => write!(
                f,
                "{MAX_ACTIVE_ROUNDS} subagents are already running; wait for their results before starting another"
            ),
            Self::Busy => {
                f.write_str("that subagent is still working; wait for its result before sending another message")
            }
        }
    }
}

impl From<RoundRefused> for String {
    fn from(refused: RoundRefused) -> Self {
        refused.to_string()
    }
}

/// Subagent conversations with a round in progress; a second message to one of them is refused.
#[derive(Clone, Default)]
pub(crate) struct BusyContexts(Arc<Mutex<HashSet<String>>>);

pub(crate) struct BusyClaim {
    contexts: BusyContexts,
    context_id: String,
}

impl BusyContexts {
    pub(crate) fn claim(&self, context_id: &str) -> Result<BusyClaim, RoundRefused> {
        if !lock(&self.0).insert(context_id.to_string()) {
            return Err(RoundRefused::Busy);
        }
        Ok(BusyClaim {
            contexts: self.clone(),
            context_id: context_id.to_string(),
        })
    }
}

impl Drop for BusyClaim {
    fn drop(&mut self) {
        lock(&self.contexts.0).remove(&self.context_id);
    }
}

/// How many subagent rounds are running, and a wake-up for callers waiting for one to end.
#[derive(Default)]
struct Rounds {
    active: AtomicUsize,
    freed: tokio::sync::Notify,
}

/// Holds one of the limited slots for running rounds until dropped.
pub(crate) struct RoundSlot(Arc<Rounds>);

impl Rounds {
    fn try_take(self: &Arc<Self>) -> Option<RoundSlot> {
        self.active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_ACTIVE_ROUNDS).then_some(n + 1)
            })
            .ok()
            .map(|_| RoundSlot(self.clone()))
    }
}

impl Drop for RoundSlot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.freed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> AgentId {
        AgentId::from(name)
    }

    #[test]
    fn a_busy_conversation_refuses_a_second_message_until_the_first_finishes() {
        let busy = BusyContexts::default();
        let claim = busy.claim("ctx").unwrap();
        assert_eq!(busy.claim("ctx").err(), Some(RoundRefused::Busy));
        assert!(busy.claim("other").is_ok());
        drop(claim);
        assert!(busy.claim("ctx").is_ok());
    }

    #[test]
    fn a_round_is_reachable_while_registered_and_reports_unread_messages_when_it_ends() {
        let runtime = SubagentRuntime::default();
        let stop = CancellationSignal::default();
        let mut round = runtime.open_round(
            &id("t"),
            &id("boss"),
            ConversationSettings::default(),
            None,
            stop.clone(),
        );

        runtime.steer(&id("t"), "check the tests").unwrap();
        assert!(runtime.stop(&id("t")) && stop.is_cancelled());
        assert_eq!(round.close(), ["check the tests"]);

        assert_eq!(runtime.steer(&id("t"), "late"), Err(SendError::NoMailbox));
        assert!(!runtime.stop(&id("t")));
        assert!(runtime.running_settings(&id("t")).is_none());
    }
}
