use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex` even if a panicking holder poisoned it: the state behind these locks stays
/// consistent across a panic, and one failed task must not take the process down with it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::lock;
    use std::sync::{Arc, Mutex};

    #[test]
    fn lock_recovers_the_value_after_a_holder_panicked() {
        let shared = Arc::new(Mutex::new(7));
        let holder = Arc::clone(&shared);
        let joined = std::thread::spawn(move || {
            let _guard = holder.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(joined.is_err());
        assert!(shared.is_poisoned());
        assert_eq!(*lock(&shared), 7);
    }
}
