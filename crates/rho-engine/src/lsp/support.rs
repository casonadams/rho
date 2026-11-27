//! Small helpers shared across the LSP modules.

pub(super) use crate::locking::lock;

pub(super) fn log_failure<T, E: std::fmt::Display>(operation: &'static str, result: Result<T, E>) {
    if let Err(err) = result {
        tracing::warn!(operation, error = %err, "LSP notification failed");
    }
}
