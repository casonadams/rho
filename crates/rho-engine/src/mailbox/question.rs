//! Questions between agents: a subagent asks its parent ([`Mailbox::ask_parent`]) and waits with
//! its work in progress; the next message anyone sends it is the answer ([`Mailboxes::tell`]), and a
//! parent that blocks or ends its turn without answering declines the questions it read
//! ([`Mailbox::decline_unanswered`]).

use super::{AgentId, Envelope, Kind, Mailbox, Mailboxes, Party, Registry, SendError};

const DECLINED: &str = "The delegating agent did not answer.";

/// How [`Mailboxes::tell`] delivered a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The recipient was waiting on a question, and the message answers it.
    Reply,
    /// Queued for the recipient's next model call.
    Queued,
}

impl Registry {
    /// Answers `asker`'s question `request` with `body` from `from`: delivers the reply, stops the
    /// asker waiting, and withdraws the question from its parent's mailbox.
    fn answer(&mut self, asker: &AgentId, request: &str, from: Party, body: &str) -> Result<(), SendError> {
        self.deliver(Envelope::reply(from, asker, request, body))?;
        let Some(slot) = self.slots.get_mut(asker) else {
            return Ok(());
        };
        slot.awaiting = None;
        let parent = slot.parent.clone();
        if let Some(parent) = parent.and_then(|parent| self.slots.get_mut(&parent)) {
            parent.queue.retain(|m| m.id != request);
        }
        Ok(())
    }
}

impl Mailboxes {
    /// Sends `body` from `from` to agent `to`: the reply to its question when it is waiting on one,
    /// otherwise a command it reads before its next model call.
    pub fn tell(&self, from: Party, to: &AgentId, body: &str) -> Result<Delivery, SendError> {
        let mut registry = self.lock();
        let awaiting = registry.slots.get(to).and_then(|slot| slot.awaiting.clone());
        let Some(request) = awaiting else {
            return registry
                .deliver(Envelope::new(from, to, Kind::Command, body))
                .map(|()| Delivery::Queued);
        };
        registry.answer(to, &request, from, body).map(|()| Delivery::Reply)
    }
}

impl Mailbox {
    /// Asks this agent's parent `question`, returning the request to wait on with [`Self::reply`].
    pub fn ask_parent(&self, question: &str) -> Result<String, SendError> {
        let mut registry = self.0.registry.lock();
        let parent = self
            .0
            .own(&mut registry)
            .ok_or(SendError::NoMailbox)?
            .parent
            .clone()
            .ok_or(SendError::NoParent)?;
        let request = Envelope::new(Party::Agent(self.id().clone()), &parent, Kind::Request, question);
        let id = request.id.clone();
        registry.deliver(request)?;
        if let Some(slot) = self.0.own(&mut registry) {
            slot.awaiting = Some(id.clone());
            slot.wake.notify_waiters();
        }
        Ok(id)
    }

    /// Stops waiting on `request` (the asker is stopping), so later messages are no longer its reply.
    pub fn withdraw(&self, request: &str) {
        self.with_slot(|slot| slot.awaiting.take_if(|awaiting| awaiting == request));
    }

    /// Waits for the reply to `request`; other messages stay queued for the next model call. A reply
    /// from [`Party::System`] means the parent declined to answer; `None` means the mailbox closed.
    pub async fn reply(&self, request: &str) -> Option<Envelope> {
        self.until(|slot| {
            let at = slot.queue.iter().position(|m| m.answers(request))?;
            slot.queue.remove(at)
        })
        .await
    }

    /// Returns once this agent is waiting on the reply to a question it asked.
    pub async fn asked(&self) {
        self.until_ready(|slot| slot.awaiting.is_some()).await;
    }

    /// Declines the questions this agent's subagents asked that it has read but not answered: it is
    /// blocking or ending its turn instead, so each asker stops waiting and pauses with its question.
    pub fn decline_unanswered(&self) {
        let mut registry = self.0.registry.lock();
        let Some(unread) = self.0.own(&mut registry).map(|slot| {
            let requests = slot.queue.iter().filter(|m| m.kind == Kind::Request);
            requests.map(|m| m.id.clone()).collect::<Vec<_>>()
        }) else {
            return;
        };
        let declined: Vec<(AgentId, String)> = registry
            .slots
            .iter()
            .filter(|(_, slot)| slot.parent.as_ref() == Some(self.id()))
            .filter_map(|(child, slot)| {
                let request = slot.awaiting.as_ref().filter(|request| !unread.contains(request))?;
                Some((child.clone(), request.clone()))
            })
            .collect();
        for (child, request) in declined {
            if let Err(error) = registry.answer(&child, &request, Party::System, DECLINED) {
                tracing::warn!("declining the question of subagent {child} failed: {error}");
            }
        }
    }

    /// Holds this agent's turn open to its subagents' questions; dropping the guard (the turn ended,
    /// however it ended) declines the ones it read but did not answer.
    #[must_use]
    pub(crate) fn answering(&self) -> Answering {
        Answering(self.clone())
    }
}

/// An agent's turn, open to its subagents' questions until dropped; see [`Mailbox::answering`].
pub(crate) struct Answering(Mailbox);

impl Drop for Answering {
    fn drop(&mut self) {
        self.0.decline_unanswered();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::MAILBOX_CAPACITY;
    use std::time::Duration;

    fn id(name: &str) -> AgentId {
        AgentId::from(name)
    }

    fn agent(name: &str) -> Party {
        Party::Agent(id(name))
    }

    #[tokio::test(start_paused = true)]
    async fn a_question_waits_for_the_next_message_to_the_asker_which_answers_it() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        let child = boxes.open(&id("child"), Some(&id("root")));
        let request = child.ask_parent("which module?").unwrap();
        boxes.notices_for(&id("child")).send("tests passed").unwrap();
        assert_eq!(
            root.read(),
            ["Message from subagent child (a reply is expected):\nwhich module?"]
        );

        let waiting = tokio::spawn({
            let (child, request) = (child.clone(), request.clone());
            async move { child.reply(&request).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "a notice is not the reply");
        assert_eq!(
            boxes.tell(agent("root"), &id("child"), "the parser"),
            Ok(Delivery::Reply)
        );

        let reply = waiting.await.unwrap().unwrap();
        assert_eq!(reply.correlation(), Some(request.as_str()));
        assert_eq!(
            child.for_model(&[reply]),
            ["Reply from the delegating agent:\nthe parser"]
        );
        assert_eq!(child.take()[0].body, "tests passed", "other messages stay queued");
        assert_eq!(
            boxes.tell(agent("root"), &id("child"), "and the lexer"),
            Ok(Delivery::Queued),
            "once answered, messages are commands again"
        );
    }

    #[test]
    fn a_parent_declines_only_the_questions_it_has_read() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        let read = boxes.open(&id("read"), Some(&id("root")));
        let unread = boxes.open(&id("unread"), Some(&id("root")));
        let _other_root = boxes.open(&id("other"), None);
        let elsewhere = boxes.open(&id("elsewhere"), Some(&id("other")));
        let read_request = read.ask_parent("seen?").unwrap();
        let _ = root.take();
        unread.ask_parent("not yet seen?").unwrap();
        elsewhere.ask_parent("another parent's?").unwrap();

        root.decline_unanswered();

        let declined = read.take();
        assert_eq!(declined.len(), 1);
        assert_eq!(
            (&declined[0].from, declined[0].correlation()),
            (&Party::System, Some(read_request.as_str()))
        );
        assert!(
            unread.take().is_empty(),
            "a question still in the parent's mailbox waits"
        );
        assert!(elsewhere.take().is_empty());
        assert_eq!(boxes.tell(agent("root"), &id("read"), "late"), Ok(Delivery::Queued));
        assert_eq!(boxes.tell(agent("root"), &id("unread"), "now"), Ok(Delivery::Reply));
    }

    #[tokio::test]
    async fn an_answered_question_is_not_also_declined_or_left_for_the_parent_to_read() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        let child = boxes.open(&id("child"), Some(&id("root")));
        let request = child.ask_parent("which module?").unwrap();

        assert_eq!(boxes.tell(Party::User, &id("child"), "the parser"), Ok(Delivery::Reply));
        root.decline_unanswered();

        assert!(
            root.take().is_empty(),
            "the answered question is withdrawn from the parent"
        );
        assert_eq!(child.reply(&request).await.unwrap().from, Party::User);
        assert!(child.take().is_empty(), "no decline follows the answer");
    }

    #[test]
    fn a_withdrawn_question_takes_no_reply_and_a_finished_turn_declines_what_it_read() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        let stopping = boxes.open(&id("stopping"), Some(&id("root")));
        let waiting = boxes.open(&id("waiting"), Some(&id("root")));
        let withdrawn = stopping.ask_parent("first?").unwrap();
        stopping.withdraw(&withdrawn);
        waiting.ask_parent("second?").unwrap();
        let _ = root.take();

        assert_eq!(boxes.tell(agent("root"), &id("stopping"), "late"), Ok(Delivery::Queued));
        drop(root.answering());

        let stopping_mail = stopping.take();
        assert_eq!((stopping_mail.len(), stopping_mail[0].kind), (1, Kind::Command));
        assert_eq!(waiting.take()[0].from, Party::System);
    }

    #[tokio::test]
    async fn a_full_mailbox_still_takes_the_awaited_answer_and_system_notices() {
        let boxes = Mailboxes::default();
        let _root = boxes.open(&id("root"), None);
        let child = boxes.open(&id("child"), Some(&id("root")));
        let request = child.ask_parent("which module?").unwrap();
        let chatter = || Envelope::new(Party::User, &id("child"), Kind::Command, "chatter");
        for _ in 0..MAILBOX_CAPACITY {
            boxes.send(chatter()).unwrap();
        }
        assert_eq!(boxes.send(chatter()), Err(SendError::Full));

        assert_eq!(boxes.notices_for(&id("child")).send("job done"), Ok(()));
        assert_eq!(
            boxes.tell(agent("root"), &id("child"), "the parser"),
            Ok(Delivery::Reply)
        );

        assert_eq!(child.reply(&request).await.unwrap().body, "the parser");
        assert_eq!(child.pending(), MAILBOX_CAPACITY + 1);
        assert_eq!(
            boxes.send(chatter()),
            Err(SendError::Full),
            "chatter is still held back"
        );
    }

    #[tokio::test]
    async fn a_decline_reaches_an_asker_whose_mailbox_is_full() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        let child = boxes.open(&id("child"), Some(&id("root")));
        let request = child.ask_parent("which module?").unwrap();
        let _ = root.take();
        for _ in 0..MAILBOX_CAPACITY {
            boxes
                .send(Envelope::new(Party::User, &id("child"), Kind::Command, "chatter"))
                .unwrap();
        }

        root.decline_unanswered();

        assert_eq!(child.reply(&request).await.unwrap().from, Party::System);
    }

    #[test]
    fn only_an_agent_with_a_parent_can_ask() {
        let boxes = Mailboxes::default();
        let root = boxes.open(&id("root"), None);
        assert_eq!(root.ask_parent("anyone?"), Err(SendError::NoParent));
        assert_eq!(boxes.tell(Party::User, &id("root"), "hi"), Ok(Delivery::Queued));
    }

    #[tokio::test]
    async fn waiting_for_a_reply_ends_when_the_mailbox_closes_or_is_replaced() {
        let boxes = Mailboxes::default();
        let _root = boxes.open(&id("root"), None);
        for close in [
            Box::new(|child: Mailbox| drop(child.close())) as Box<dyn Fn(Mailbox)>,
            Box::new(|child: Mailbox| drop(child.registry().open(child.id(), Some(&id("root"))))),
        ] {
            let child = boxes.open(&id("child"), Some(&id("root")));
            let request = child.ask_parent("which module?").unwrap();
            let waiting = tokio::spawn({
                let (child, request) = (child.clone(), request.clone());
                async move { child.reply(&request).await }
            });
            tokio::task::yield_now().await;

            close(child);

            let reply = tokio::time::timeout(Duration::from_secs(1), waiting).await;
            assert!(reply.expect("the waiter woke").unwrap().is_none());
        }
    }
}
