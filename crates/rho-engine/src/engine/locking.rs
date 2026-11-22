use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex` even if a panicking holder poisoned it: the state behind these locks stays
/// consistent across a panic, and one failed task must not take the engine down with it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
