//! Per-agent mailboxes: every agent, at any depth, is an actor with one bounded queue of typed
//! envelopes, addressed by agent id. Where an agent sits in the delegation tree decides who it may
//! message and how its messages are introduced to the reader ([`Relation`]).
//!
//! Steering, messages to a running subagent, replies to questions, and background notices all travel
//! as [`Envelope`]s. An agent reads its [`Mailbox`] at a safe point (before its next model call); a
//! wake signal only tells a waiting reader to look, so a missed wake-up never loses a message.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::Notify;

/// Messages a mailbox holds before further sends are refused.
pub const MAILBOX_CAPACITY: usize = 32;
/// Longest chain of replies to replies; stops two agents from answering each other forever.
pub const MAX_HOPS: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    User,
    System,
    Agent(String),
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
    pub to: String,
    pub kind: Kind,
    pub body: String,
    /// For a reply, the id of the request it answers.
    pub correlation: Option<String>,
    /// How many replies deep this message is.
    pub hops: u8,
    /// How an agent sender relates to the recipient, set when the message is sent.
    relation: Option<Relation>,
}

impl Envelope {
    #[must_use]
    pub fn new(from: Party, to: impl Into<String>, kind: Kind, body: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to: to.into(),
            kind,
            body: body.into(),
            correlation: None,
            hops: 0,
            relation: None,
        }
    }

    /// An answer to `request`, addressed to the agent that asked; `None` when the asker is not an agent.
    #[must_use]
    pub fn reply_to(request: &Self, from: Party, body: impl Into<String>) -> Option<Self> {
        let Party::Agent(asker) = &request.from else {
            return None;
        };
        let mut reply = Self::new(from, asker.clone(), Kind::Reply, body);
        reply.correlation = Some(request.id.clone());
        reply.hops = request.hops.saturating_add(1);
        Some(reply)
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
    #[must_use]
    pub fn heading(&self) -> String {
        let source = match (&self.from, self.relation) {
            (Party::User, _) => "Message from the user".to_string(),
            (Party::System, _) => "System message".to_string(),
            (Party::Agent(_), Some(Relation::Parent)) => "Message from the delegating agent".to_string(),
            (Party::Agent(id), Some(Relation::Child)) => format!("Message from subagent {id}"),
            (Party::Agent(id), Some(Relation::Sibling) | None) => format!("Message from agent {id}"),
        };
        match self.kind {
            Kind::Request => format!("{source} (a reply is expected)"),
            Kind::Command | Kind::Event | Kind::Reply => source,
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
    TooManyHops,
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
            Self::TooManyHops => write!(f, "the reply chain is longer than {MAX_HOPS} replies"),
        }
    }
}

impl std::error::Error for SendError {}

struct Slot {
    generation: u64,
    parent: Option<String>,
    queue: VecDeque<Envelope>,
    wake: Arc<Notify>,
}

#[derive(Default)]
struct Registry {
    next_generation: u64,
    slots: HashMap<String, Slot>,
}

/// How agent `from` relates to agent `to`, given who `to`'s parent is; `None` when `from` may not
/// message `to`.
fn relation(registry: &Registry, from: &str, to: &str, to_parent: Option<&str>) -> Option<Relation> {
    let sender_parent = registry.slots.get(from)?.parent.as_deref();
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

/// The mailboxes of every running agent of an engine.
#[derive(Clone, Default)]
pub struct Mailboxes(Arc<Mutex<Registry>>);

impl Mailboxes {
    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens agent `id`'s mailbox, replacing any earlier one; `parent` is the agent that delegated to it.
    pub fn open(&self, id: &str, parent: Option<&str>) -> Mailbox {
        let mut registry = self.lock();
        registry.next_generation += 1;
        let generation = registry.next_generation;
        let wake = Arc::new(Notify::new());
        registry.slots.insert(
            id.to_string(),
            Slot {
                generation,
                parent: parent.map(str::to_string),
                queue: VecDeque::new(),
                wake: wake.clone(),
            },
        );
        Mailbox(Arc::new(Handle {
            registry: self.clone(),
            id: id.to_string(),
            generation,
            wake,
        }))
    }

    #[must_use]
    pub fn is_open(&self, id: &str) -> bool {
        self.lock().slots.contains_key(id)
    }

    pub fn send(&self, mut envelope: Envelope) -> Result<(), SendError> {
        if envelope.hops > MAX_HOPS {
            return Err(SendError::TooManyHops);
        }
        let mut registry = self.lock();
        let target = registry.slots.get(&envelope.to).ok_or(SendError::NoMailbox)?;
        if let Party::Agent(from) = &envelope.from {
            envelope.relation =
                Some(relation(&registry, from, &envelope.to, target.parent.as_deref()).ok_or(SendError::NotAllowed)?);
        }
        let slot = registry.slots.get_mut(&envelope.to).ok_or(SendError::NoMailbox)?;
        if slot.queue.len() >= MAILBOX_CAPACITY {
            return Err(SendError::Full);
        }
        slot.queue.push_back(envelope);
        slot.wake.notify_waiters();
        Ok(())
    }
}

/// An agent's own end of its mailbox. Clones read the same queue; dropping the last one closes it.
#[derive(Clone)]
pub struct Mailbox(Arc<Handle>);

struct Handle {
    registry: Mailboxes,
    id: String,
    generation: u64,
    wake: Arc<Notify>,
}

impl Mailbox {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// The registry this mailbox belongs to, for sending to other agents.
    #[must_use]
    pub fn registry(&self) -> &Mailboxes {
        &self.0.registry
    }

    fn with_slot<R>(&self, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
        let mut registry = self.0.registry.lock();
        registry
            .slots
            .get_mut(&self.0.id)
            .filter(|slot| slot.generation == self.0.generation)
            .map(f)
    }

    /// The agent that delegated to this one, if any.
    #[must_use]
    pub fn parent(&self) -> Option<String> {
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

    /// The waiting system notices, oldest first; messages from users and agents stay queued.
    #[must_use]
    pub fn take_notices(&self) -> Vec<Envelope> {
        self.with_slot(|slot| {
            let all: Vec<Envelope> = slot.queue.drain(..).collect();
            let (notices, rest): (Vec<_>, Vec<_>) = all
                .into_iter()
                .partition(|m| m.from == Party::System && m.kind == Kind::Event);
            slot.queue = rest.into();
            notices
        })
        .unwrap_or_default()
    }

    #[must_use]
    pub fn pending(&self) -> usize {
        self.with_slot(|slot| slot.queue.len()).unwrap_or(0)
    }

    /// Returns once at least one message is waiting.
    pub async fn wait(&self) {
        crate::wake::until(&self.0.wake, || (self.pending() > 0).then_some(())).await;
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
    fn shutdown(&self) -> Vec<Envelope> {
        let mut registry = self.registry.lock();
        let ours = registry
            .slots
            .get(&self.id)
            .is_some_and(|slot| slot.generation == self.generation);
        if !ours {
            return Vec::new();
        }
        registry
            .slots
            .remove(&self.id)
            .map(|slot| slot.queue.into_iter().collect())
            .unwrap_or_default()
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

    fn agent(id: &str) -> Party {
        Party::Agent(id.to_string())
    }

    fn command(from: Party, to: &str, body: &str) -> Envelope {
        Envelope::new(from, to, Kind::Command, body)
    }

    #[test]
    fn messages_arrive_in_order_and_are_taken_once() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open("a", None);
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
        let mailbox = boxes.open("a", None);
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
        let mailbox = boxes.open("a", None);
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
        let mailbox = boxes.open("a", None);
        boxes.send(command(Party::User, "a", "unread")).unwrap();

        let unread = mailbox.close();

        assert_eq!(unread.len(), 1);
        assert!(!boxes.is_open("a"));
        assert_eq!(boxes.send(command(Party::User, "a", "late")), Err(SendError::NoMailbox));
    }

    #[test]
    fn an_old_handle_closing_leaves_the_mailbox_that_replaced_it_open() {
        let boxes = Mailboxes::default();
        let old = boxes.open("task", None);
        let new = boxes.open("task", None);
        boxes.send(command(Party::User, "task", "for the new round")).unwrap();

        assert!(old.take().is_empty(), "the replaced handle no longer reads");
        drop(old);

        assert!(boxes.is_open("task"));
        assert_eq!(new.take().len(), 1);
    }

    #[test]
    fn agents_may_message_their_parent_children_and_siblings_only() {
        let boxes = Mailboxes::default();
        let _root = boxes.open("root", None);
        let _a = boxes.open("a", Some("root"));
        let _b = boxes.open("b", Some("root"));
        let _a_child = boxes.open("a1", Some("a"));
        let _stranger = boxes.open("other-root", None);
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
    fn replies_route_back_to_the_asker_and_stop_after_too_many_hops() {
        let boxes = Mailboxes::default();
        let _root = boxes.open("root", None);
        let _child = boxes.open("child", Some("root"));
        let question = Envelope::new(agent("child"), "root", Kind::Request, "which module?");
        boxes.send(question.clone()).unwrap();

        let reply = Envelope::reply_to(&question, agent("root"), "the parser").unwrap();

        assert_eq!((reply.to.as_str(), reply.kind, reply.hops), ("child", Kind::Reply, 1));
        assert_eq!(reply.correlation.as_deref(), Some(question.id.as_str()));
        assert_eq!(boxes.send(reply.clone()), Ok(()));
        assert!(Envelope::reply_to(&command(Party::User, "root", "x"), agent("root"), "?").is_none());

        let mut chain = reply;
        for _ in 0..MAX_HOPS {
            let (from, to) = (chain.to.clone(), chain.from.clone());
            chain = Envelope::reply_to(&chain, Party::Agent(from), "again").unwrap();
            assert_eq!(to, Party::Agent(chain.to.clone()));
        }
        assert_eq!(boxes.send(chain), Err(SendError::TooManyHops));
    }

    #[test]
    fn clones_share_the_queue_and_only_the_last_drop_closes_it() {
        let boxes = Mailboxes::default();
        let mailbox = boxes.open("a", Some("root"));
        let clone = mailbox.clone();
        boxes.send(command(Party::User, "a", "x")).unwrap();

        assert_eq!(clone.take().len(), 1);
        assert_eq!(mailbox.parent().as_deref(), Some("root"));
        drop(mailbox);
        assert!(boxes.is_open("a"), "a clone still holds it open");
        drop(clone);
        assert!(!boxes.is_open("a"));
    }

    #[test]
    fn the_model_sees_notices_and_the_users_words_to_the_top_agent_unframed() {
        let boxes = Mailboxes::default();
        let top = boxes.open("boss", None);
        let child = boxes.open("a", Some("boss"));
        for (party, kind) in [(Party::System, Kind::Event), (Party::User, Kind::Command)] {
            boxes.send(Envelope::new(party.clone(), "boss", kind, "body")).unwrap();
            boxes.send(Envelope::new(party, "a", kind, "body")).unwrap();
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
        let _boss = boxes.open("boss", None);
        let a = boxes.open("a", Some("boss"));
        let _b = boxes.open("b", Some("boss"));
        let _a1 = boxes.open("a1", Some("a"));
        let cases = [
            (command(Party::User, "a", "x"), "Message from the user"),
            (Envelope::new(Party::System, "a", Kind::Event, "x"), "System message"),
            (command(agent("boss"), "a", "x"), "Message from the delegating agent"),
            (command(agent("b"), "a", "x"), "Message from agent b"),
            (command(agent("a1"), "a", "x"), "Message from subagent a1"),
            (
                Envelope::new(agent("boss"), "a", Kind::Request, "x"),
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
