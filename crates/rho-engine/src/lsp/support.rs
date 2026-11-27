//! Small helpers shared across the LSP modules.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, continuing with the data a panicking holder left behind.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn log_failure<T, E: std::fmt::Display>(operation: &'static str, result: Result<T, E>) {
    if let Err(err) = result {
        tracing::warn!(operation, error = %err, "LSP notification failed");
    }
}
