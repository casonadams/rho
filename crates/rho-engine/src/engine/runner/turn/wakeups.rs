//! What an agent waiting on its background work watches: its mailbox, the user's steering, and the
//! background jobs of its engine.

use super::types::SteeringQueueProvider;
use crate::mailbox::Mailbox;
use tokio::sync::watch;

/// Whether a message from the agent's mailbox or the user's steering waits for its next model call.
pub(crate) fn message_waiting(inbox: &Mailbox, steering: Option<&dyn SteeringQueueProvider>) -> bool {
    inbox.pending() > 0 || steering.is_some_and(SteeringQueueProvider::has_steering)
}

/// What wakes an agent waiting on its background work: a message in its mailbox, steering from the
/// user, or any job of its engine ending. Made before the waiter first checks what it waits for, so
/// it never misses a change that happens between that check and the wait.
pub(crate) struct Wakeups<'a> {
    inbox: &'a Mailbox,
    steering: Option<&'a dyn SteeringQueueProvider>,
    ended: watch::Receiver<u64>,
}

impl<'a> Wakeups<'a> {
    pub(crate) fn new(inbox: &'a Mailbox, steering: Option<&'a dyn SteeringQueueProvider>) -> Self {
        Self {
            inbox,
            steering,
            ended: inbox.jobs().watch_ended(),
        }
    }

    /// Returns once a message or steering is waiting, or after a job ended since the last call. An
    /// agent that blocks here is not answering, so it first declines the questions it has read.
    pub(crate) async fn next(&mut self) {
        let Self { inbox, steering, ended } = self;
        inbox.decline_unanswered();
        let job_ended = async {
            if ended.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        let steering = async {
            match steering {
                Some(steering) => steering.arrived().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = job_ended => {}
            () = inbox.wait() => {}
            () = steering => {}
        }
    }
}
