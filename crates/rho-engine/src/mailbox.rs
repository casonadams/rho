//! Per-agent mailboxes: every agent, at any depth, is an actor with one bounded queue of typed
//! envelopes, addressed by agent id. Where an agent sits in the delegation tree decides who it may
//! message and how its messages are introduced to the reader ([`Relation`]).
//!
//! Steering, messages to a running subagent, questions and their replies (see `question`), and
//! background notices (see `notice`) all travel as [`Envelope`]s. An agent reads its [`Mailbox`] at a
//! safe point (before its next model call); a wake signal only tells a waiting reader to look, so a
//! missed wake-up never loses a message. The background work an agent owns is tracked with the
//! mailboxes, so the agent reaches it through its own mailbox.

use crate::tools::jobs::{BackgroundJobInfo, JobCompletion, JobId, JobManager};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::Notify;

mod notice;
mod question;

pub use notice::NoticeTarget;
pub(crate) use notice::{framed_notice, neutralize_notice_tags};
pub use question::{Answering, Delivery};

/// Messages a mailbox holds before further sends are refused.
pub const MAILBOX_CAPACITY: usize = 32;

crate::id::string_id!(
    /// Names one agent: a top-level agent by its session id, a subagent by its task id.
    AgentId
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    User,
    System,
    Agent(AgentId),
}

/// What the recipient is expected to do with a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An instruction to act on.
    Command,
    /// A report that something happened; it needs no action.
    Event,
    /// A question that waits for a [`Kind::Reply`] carrying its id as the correlation.
    Request,
    Reply,
}

/// Where an agent sender sits relative to the recipient in the delegation tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The sender delegated to the recipient.
    Parent,
    /// The recipient delegated to the sender.
    Child,
    /// Both were delegated to by the same agent, or both are top-level agents.
    Sibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub id: String,
    pub from: Party,
    /// The recipient's agent id.
    pub to: AgentId,
    pub kind: Kind,
    pub body: String,
    /// For a reply, the id of the request it answers; set only by [`Envelope::reply`].
    correlation: Option<String>,
    /// How an agent sender relates to the recipient, set when the message is sent.
    relation: Option<Relation>,
}

impl Envelope {
    #[must_use]
    pub fn new(from: Party, to: &AgentId, kind: Kind, body: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to: to.clone(),
            kind,
            body: body.into(),
            correlation: None,
            relation: None,
        }
    }

    /// The answer to request `request` from agent `to`.
    #[must_use]
    pub fn reply(from: Party, to: &AgentId, request: &str, body: impl Into<String>) -> Self {
        let mut reply = Self::new(from, to, Kind::Reply, body);
        reply.correlation = Some(request.to_string());
        reply
    }

    /// A system notice for agent `to`: a report it reads, like a finished background job's.
    #[must_use]
    pub fn notice(to: &AgentId, body: impl Into<String>) -> Self {
        Self::new(Party::System, to, Kind::Event, body)
    }

    #[must_use]
    pub fn is_notice(&self) -> bool {
        self.from == Party::System && self.kind == Kind::Event
    }

    /// For a reply, the id of the request it answers.
    #[must_use]
    pub fn correlation(&self) -> Option<&str> {
        self.correlation.as_deref()
    }

    fn answers(&self, request: &str) -> bool {
        self.kind == Kind::Reply && self.correlation() == Some(request)
    }

    /// How the message is shown to a model; `top_level` says no agent delegated to the reader. System
    /// notices carry their own framing, and the user speaks plainly to the top-level agent.
    #[must_use]
    pub fn for_model(&self, top_level: bool) -> String {
        let plain = match self.from {
            Party::System => true,
            Party::User => top_level,
            Party::Agent(_) => false,
        };
        if plain && self.kind != Kind::Request {
            return self.body.clone();
        }
        format!("{}:\n{}", self.heading(), self.body.trim())
    }

    /// How the message is introduced to the model, by who sent it relative to the reader.
    fn heading(&self) -> String {
        let sender = match (&self.from, self.relation) {
            (Party::System, _) => return "System message".to_string(),
            (Party::User, _) => "the user".to_string(),
            (Party::Agent(_), Some(Relation::Parent)) => "the delegating agent".to_string(),
            (Party::Agent(id), Some(Relation::Child)) => format!("subagent {id}"),
            (Party::Agent(id), Some(Relation::Sibling) | None) => format!("agent {id}"),
        };
        match self.kind {
            Kind::Request => format!("Message from {sender} (a reply is expected)"),
            Kind::Reply => format!("Reply from {sender}"),
            Kind::Command | Kind::Event => format!("Message from {sender}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The recipient has no open mailbox: unknown, or its round has ended.
    NoMailbox,
    /// An agent may message its parent, its children, and its siblings, and nobody else.
    NotAllowed,
    Full,
    /// A top-level agent has nobody to ask.
    NoParent,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMailbox => f.write_str("the recipient is not running"),
            Self::NotAllowed => f.write_str("an agent may message only its parent, its subagents, and its siblings"),
            Self::Full => write!(
                f,
                "the recipient already has {MAILBOX_CAPACITY} unread messages; wait for it to read them"
            ),
            Self::NoParent => f.write_str("no agent delegated to this one"),
        }
    }
}

impl std::error::Error for SendError {}

struct Slot {
    generation: u64,
    parent: Option<AgentId>,
    queue: VecDeque<Envelope>,
    wake: Arc<Notify>,
    /// The request this agent asked and is waiting on a reply to.
    awaiting: Option<String>,
}

#[derive(Default)]
struct Registry {
    next_generation: u64,
    slots: HashMap<AgentId, Slot>,
}

/// How agent `from` relates to agent `to`, given who `to`'s parent is; `None` when `from` may not
/// message `to`.
fn relation(registry: &Registry, from: &AgentId, to: &AgentId, to_parent: Option<&AgentId>) -> Option<Relation> {
    let sender_parent = registry.slots.get(from)?.parent.as_ref();
    if from == to {
        None
    } else if to_parent == Some(from) {
        Some(Relation::Parent)
    } else if sender_parent == Some(to) {
        Some(Relation::Child)
    } else {
        (sender_parent == to_parent).then_some(Relation::Sibling)
    }
}

impl Registry {
    /// Queues `envelope` for its recipient, stamping how an agent sender relates to it.
    fn deliver(&mut self, mut envelope: Envelope) -> Result<(), SendError> {
        let target = self.slots.get(&envelope.to).ok_or(SendError::NoMailbox)?;
        if let Party::Agent(from) = &envelope.from {
            envelope.relation =
                Some(relation(self, from, &envelope.to, target.parent.as_ref()).ok_or(SendError::NotAllowed)?);
        }
        let slot = self.slots.get_mut(&envelope.to).ok_or(SendError::NoMailbox)?;
        if slot.queue.len() >= MAILBOX_CAPACITY {
            return Err(SendError::Full);
        }
        slot.queue.push_back(envelope);
        slot.wake.notify_waiters();
        Ok(())
    }
}

/// The mailboxes of every running agent of an engine, and the background work those agents own.
#[derive(Clone, Default)]
pub struct Mailboxes {
    registry: Arc<Mutex<Registry>>,
    jobs: JobManager,
}

impl Mailboxes {
    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The background work of this engine's agents.
    #[must_use]
    pub fn jobs(&self) -> &JobManager {
        &self.jobs
    }

    /// Starts tracking background work agent `owner` owns; its notice goes to `owner`.
    pub(crate) fn start_job(&self, owner: &AgentId, id: JobId, command: String) -> JobCompletion {
        self.jobs.start(Some(self.notices_for(owner)), id, command)
    }

    /// Opens agent `id`'s mailbox, replacing any earlier one; `parent` is the agent that delegated to it.
    pub fn open(&self, id: &AgentId, parent: Option<&AgentId>) -> Mailbox {
        let mut registry = self.lock();
        registry.next_generation += 1;
        let generation = registry.next_generation;
        let wake = Arc::new(Notify::new());
        let replaced = registry.slots.insert(
            id.clone(),
            Slot {
                generation,
                parent: parent.cloned(),
                queue: VecDeque::new(),
                wake: wake.clone(),
                awaiting: None,
            },
        );
        if let Some(replaced) = replaced {
            replaced.wake.notify_waiters();
        }
        Mailbox(Arc::new(Handle {
            registry: self.clone(),
            id: id.clone(),
            generation,
            wake,
        }))
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self, id: &AgentId) -> bool {
        self.lock().slots.contains_key(id)
    }

    /// Where agent `agent`'s notices go, whether or not its mailbox is open yet.
    #[must_use]
    pub fn notices_for(&self, agent: &AgentId) -> NoticeTarget {
        NoticeTarget::new(self.clone(), agent)
    }

    pub fn send(&self, envelope: Envelope) -> Result<(), SendError> {
        self.lock().deliver(envelope)
    }
}

/// An agent's own end of its mailbox. Clones read the same queue; dropping the last one closes it.
#[derive(Clone)]
pub struct Mailbox(Arc<Handle>);

struct Handle {
    registry: Mailboxes,
    id: AgentId,
    generation: u64,
    wake: Arc<Notify>,
}

impl Mailbox {
    #[must_use]
    pub fn id(&self) -> &AgentId {
        &self.0.id
    }

    /// The registry this mailbox belongs to, for sending to other agents.
    #[must_use]
    pub fn registry(&self) -> &Mailboxes {
        &self.0.registry
    }

    /// Where this agent's notices go; holding it does not keep the mailbox open.
    #[must_use]
    pub fn notices(&self) -> NoticeTarget {
        self.0.registry.notices_for(&self.0.id)
    }

    /// The background work of this agent's engine; [`Self::start_job`] adds work this agent owns.
    #[must_use]
    pub fn jobs(&self) -> &JobManager {
        self.0.registry.jobs()
    }

    /// Starts tracking background work this agent owns; its notice comes to this mailbox.
    pub fn start_job(&self, id: JobId, command: String) -> JobCompletion {
        self.0.registry.start_job(self.id(), id, command)
    }

    /// This agent's running jobs, oldest first.
    #[must_use]
    pub fn running_jobs(&self) -> Vec<BackgroundJobInfo> {
        self.jobs().running(self.id())
    }

    #[must_use]
    pub fn has_running_jobs(&self) -> bool {
        self.jobs().has_running(self.id())
    }

    /// Signals every running job this agent owns to stop.
    pub fn cancel_jobs(&self) {
        self.jobs().cancel_owner(self.id());
    }

    fn with_slot<R>(&self, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
        let mut registry = self.0.registry.lock();
        self.0.own(&mut registry).map(f)
    }

    /// The agent that delegated to this one, if any.
    fn parent(&self) -> Option<AgentId> {
        self.with_slot(|slot| slot.parent.clone()).flatten()
    }

    /// `envelopes` as this agent's model is shown them.
    #[must_use]
    pub fn for_model(&self, envelopes: &[Envelope]) -> Vec<String> {
        let top_level = self.parent().is_none();
        envelopes.iter().map(|e| e.for_model(top_level)).collect()
    }

    /// Every message waiting, oldest first.
    #[must_use]
    pub fn take(&self) -> Vec<Envelope> {
        self.with_slot(|slot| slot.queue.drain(..).collect())
            .unwrap_or_default()
    }

    /// Takes every waiting message as this agent's model is shown them.
    #[must_use]
    pub fn read(&self) -> Vec<String> {
        self.for_model(&self.take())
    }

    /// Takes the waiting system notices as the model is shown them; other messages stay queued.
    #[must_use]
    pub fn read_notices(&self) -> Vec<String> {
        let notices = self
            .with_slot(|slot| {
                let (notices, rest): (Vec<_>, Vec<_>) = slot.queue.drain(..).partition(Envelope::is_notice);
                slot.queue = rest.into();
                notices
            })
            .unwrap_or_default();
        self.for_model(&notices)
    }

    #[must_use]
    pub fn pending(&self) -> usize {
        self.with_slot(|slot| slot.queue.len()).unwrap_or(0)
    }

    /// Returns once at least one message is waiting; never returns once the mailbox is closed.
    pub async fn wait(&self) {
        if self
            .until(|slot| (!slot.queue.is_empty()).then_some(()))
            .await
            .is_none()
        {
            std::future::pending::<()>().await;
        }
    }

    /// Returns what `ready` finds in this agent's slot, re-checking whenever a message arrives, or
    /// `None` once the mailbox is closed or replaced.
    async fn until<T>(&self, mut ready: impl FnMut(&mut Slot) -> Option<T>) -> Option<T> {
        crate::wake::until(&self.0.wake, || match self.with_slot(&mut ready) {
            None => Some(None),
            Some(found) => found.map(Some),
        })
        .await
    }

    /// Closes the mailbox for every clone, returning the messages the agent never read.
    #[must_use]
    pub fn close(self) -> Vec<Envelope> {
        self.0.shutdown()
    }

    /// Whether `other` is a handle to this same mailbox.
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Handle {
    /// This handle's slot in `registry`, unless the mailbox was closed or replaced.
    fn own<'a>(&self, registry: &'a mut Registry) -> Option<&'a mut Slot> {
        registry
            .slots
            .get_mut(&self.id)
            .filter(|slot| slot.generation == self.generation)
    }

    /// Removes this handle's slot, waking anyone waiting on it, and returns its unread messages.
    fn shutdown(&self) -> Vec<Envelope> {
        let mut registry = self.registry.lock();
        if self.own(&mut registry).is_none() {
            return Vec::new();
        }
        let Some(slot) = registry.slots.remove(&self.id) else {
            return Vec::new();
        };
        slot.wake.notify_waiters();
        slot.queue.into()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn id(name: &str) -> AgentId {
        AgentId::from(name)
    }

    fn agent(name: &str) -> Party {
        Party::Agent(id(name))
    }

    fn command(from: Party, to: &str, body: &str) -> Envelope {
        Envelope::new(from, &id(to), Kind::Command, body)
    }

    #[test]
    fn messages_arrive_in_order_and_are_taken_once() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open(&id("a"), None);
        for body in ["one", "two", "three"] {
            boxes.send(command(Party::User, "a", body)).unwrap();
        }

        let bodies: Vec<String> = mailbox.take().into_iter().map(|m| m.body).collect();

        assert_eq!(bodies, ["one", "two", "three"]);
        assert!(mailbox.take().is_empty());
        assert_eq!(mailbox.pending(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiting_reader_wakes_for_a_message_and_a_message_sent_earlier_is_never_missed() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open(&id("a"), None);
        boxes.send(command(Party::User, "a", "early")).unwrap();
        tokio::time::timeout(Duration::from_secs(1), mailbox.wait())
            .await
            .expect("a waiting message returns at once");
        let _ = mailbox.take();

        let waiter = tokio::spawn(async move {
            mailbox.wait().await;
            mailbox.take()
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        boxes.send(command(Party::System, "a", "late")).unwrap();

        let got = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "late");
    }

    #[test]
    fn a_full_mailbox_refuses_more_until_it_is_read() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open(&id("a"), None);
        for _ in 0..MAILBOX_CAPACITY {
            boxes.send(command(Party::User, "a", "x")).unwrap();
        }
        assert_eq!(boxes.send(command(Party::User, "a", "x")), Err(SendError::Full));

        let _ = mailbox.take();

        assert_eq!(boxes.send(command(Party::User, "a", "x")), Ok(()));
    }

    #[test]
    fn closing_returns_unread_messages_and_refuses_new_ones() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open(&id("a"), None);
        boxes.send(command(Party::User, "a", "unread")).unwrap();

        let unread = mailbox.close();

        assert_eq!(unread.len(), 1);
        assert!(!boxes.is_open(&id("a")));
        assert_eq!(boxes.send(command(Party::User, "a", "late")), Err(SendError::NoMailbox));
    }

    #[test]
    fn an_old_handle_closing_leaves_the_mailbox_that_replaced_it_open() {
        let boxes = Mailboxes::default();
        let old = boxes.open(&id("task"), None);
        let new = boxes.open(&id("task"), None);
        boxes.send(command(Party::User, "task", "for the new round")).unwrap();

        assert!(old.take().is_empty(), "the replaced handle no longer reads");
        drop(old);

        assert!(boxes.is_open(&id("task")));
        assert_eq!(new.take().len(), 1);
    }

    #[test]
    fn agents_may_message_their_parent_children_and_siblings_only() {
        let boxes = Mailboxes::default();
        let _root = boxes.open(&id("root"), None);
        let _a = boxes.open(&id("a"), Some(&id("root")));
        let _b = boxes.open(&id("b"), Some(&id("root")));
        let _a_child = boxes.open(&id("a1"), Some(&id("a")));
        let _stranger = boxes.open(&id("other-root"), None);
        let allowed = [
            ("root", "a"),
            ("a", "root"),
            ("a", "b"),
            ("b", "a"),
            ("a", "a1"),
            ("a1", "a"),
        ];
        let denied = [
            ("a1", "root"),
            ("root", "a1"),
            ("a1", "b"),
            ("a", "other-root"),
            ("other-root", "a"),
            ("a", "a"),
            ("ghost", "a"),
        ];

        for (from, to) in allowed {
            assert_eq!(boxes.send(command(agent(from), to, "hi")), Ok(()), "{from} -> {to}");
        }
        for (from, to) in denied {
            assert_eq!(
                boxes.send(command(agent(from), to, "hi")),
                Err(SendError::NotAllowed),
                "{from} -> {to}"
            );
        }
        assert_eq!(
            boxes.send(command(agent("a"), "missing", "hi")),
            Err(SendError::NoMailbox)
        );
        assert_eq!(
            boxes.send(command(Party::User, "a1", "hi")),
            Ok(()),
            "the user may reach any agent"
        );
    }

    #[test]
    fn clones_share_the_queue_and_only_the_last_drop_closes_it() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open(&id("a"), Some(&id("root")));
        let clone = mailbox.clone();
        boxes.send(command(Party::User, "a", "x")).unwrap();

        assert_eq!(clone.take().len(), 1);
        assert_eq!(mailbox.parent(), Some(id("root")));
        drop(mailbox);
        assert!(boxes.is_open(&id("a")), "a clone still holds it open");
        drop(clone);
        assert!(!boxes.is_open(&id("a")));
    }

    #[test]
    fn the_model_sees_notices_and_the_users_words_to_the_top_agent_unframed() {
        let boxes = Mailboxes::default();
        let top = boxes.open(&id("boss"), None);
        let child = boxes.open(&id("a"), Some(&id("boss")));
        for (party, kind) in [(Party::System, Kind::Event), (Party::User, Kind::Command)] {
            boxes
                .send(Envelope::new(party.clone(), &id("boss"), kind, "body"))
                .unwrap();
            boxes.send(Envelope::new(party, &id("a"), kind, "body")).unwrap();
        }
        boxes.send(command(agent("boss"), "a", "body")).unwrap();

        assert_eq!(top.for_model(&top.take()), ["body", "body"]);
        assert_eq!(
            child.for_model(&child.take()),
            [
                "body",
                "Message from the user:\nbody",
                "Message from the delegating agent:\nbody"
            ]
        );
    }

    #[test]
    fn headings_say_how_the_sender_relates_to_the_reader() {
        let boxes = Mailboxes::default();
        let _boss = boxes.open(&id("boss"), None);
        let a = boxes.open(&id("a"), Some(&id("boss")));
        let _b = boxes.open(&id("b"), Some(&id("boss")));
        let _a1 = boxes.open(&id("a1"), Some(&id("a")));
        let cases = [
            (command(Party::User, "a", "x"), "Message from the user"),
            (
                Envelope::new(Party::System, &id("a"), Kind::Event, "x"),
                "System message",
            ),
            (command(agent("boss"), "a", "x"), "Message from the delegating agent"),
            (command(agent("b"), "a", "x"), "Message from agent b"),
            (command(agent("a1"), "a", "x"), "Message from subagent a1"),
            (
                Envelope::new(agent("boss"), &id("a"), Kind::Request, "x"),
                "Message from the delegating agent (a reply is expected)",
            ),
        ];
        for (envelope, _) in &cases {
            boxes.send(envelope.clone()).unwrap();
        }

        let headings: Vec<String> = a.take().iter().map(Envelope::heading).collect();

        let expected: Vec<&str> = cases.iter().map(|(_, heading)| *heading).collect();
        assert_eq!(headings, expected);
        assert_eq!(command(agent("boss"), "a", "x").heading(), "Message from agent boss");
    }

    #[test]
    fn send_errors_explain_the_refusal() {
        assert_eq!(
            SendError::Full.to_string(),
            format!("the recipient already has {MAILBOX_CAPACITY} unread messages; wait for it to read them")
        );
        assert_eq!(SendError::NoMailbox.to_string(), "the recipient is not running");
    }
}
