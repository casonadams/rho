//! Spinner/activity lifetime token held while the model is thinking or a tool
//! is running. The presentation layer supplies the closure invoked on finish.

use crate::locking::lock;
use std::sync::{Arc, Mutex};

type Finisher = std::boxed::Box<dyn FnOnce() + Send>;
type FinisherSlot = Arc<Mutex<Option<Finisher>>>;

#[derive(Clone, Default)]
pub struct ActivityToken {
    finisher: FinisherSlot,
}

impl ActivityToken {
    pub fn finish_and_clear(self) {
        self.run_finisher();
    }

    pub fn disarm(&self) {
        lock(&self.finisher).take();
    }

    fn run_finisher(&self) {
        let finish = lock(&self.finisher).take();
        if let Some(finish) = finish {
            finish();
        }
    }
}

impl Drop for ActivityToken {
    fn drop(&mut self) {
        if Arc::strong_count(&self.finisher) == 1 {
            self.run_finisher();
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

#[cfg(test)]
mod tests;
