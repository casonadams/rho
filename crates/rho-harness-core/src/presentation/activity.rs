//! Spinner/activity lifetime token held while the model is thinking or a tool
//! is running. The presentation layer supplies the closure invoked on finish.

use std::sync::{Arc, Mutex};

type Finisher = std::boxed::Box<dyn FnOnce() + Send>;
type FinisherSlot = Arc<Mutex<Option<Finisher>>>;

#[derive(Clone, Default)]
pub struct ActivityToken {
    finisher: FinisherSlot,
}

impl ActivityToken {
    pub fn finish_and_clear(self) {
        if let Ok(mut slot) = self.finisher.try_lock()
            && let Some(finish) = slot.take()
        {
            finish();
        }
    }
}

/// Build an activity token from a one-shot finisher closure.
pub fn activity_token<F>(finisher: F) -> ActivityToken
where
    F: FnOnce() + Send + 'static,
{
    ActivityToken {
        finisher: Arc::new(Mutex::new(Some(Box::new(finisher)))),
    }
}
