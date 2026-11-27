use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Locks `mutex` even if a panicking holder poisoned it: the state behind these locks stays
/// consistent across a panic, and one failed task must not take the process down with it.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes the value out of `mutex`, recovering it like [`lock`] if a panicking holder poisoned it.
pub fn into_inner<T>(mutex: Mutex<T>) -> T {
    mutex.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// Read-locks `rwlock` the way [`lock`] locks a mutex.
pub fn read<T>(rwlock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    rwlock.read().unwrap_or_else(PoisonError::into_inner)
}

/// Write-locks `rwlock` the way [`lock`] locks a mutex.
pub fn write<T>(rwlock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    rwlock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{into_inner, lock, read, write};
    use std::sync::{Arc, Mutex, RwLock};

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
        let owned = Arc::into_inner(shared).expect("the holder thread released its handle");
        assert_eq!(into_inner(owned), 7);
    }

    #[test]
    fn read_and_write_recover_the_value_after_a_holder_panicked() {
        let shared = Arc::new(RwLock::new(7));
        let holder = Arc::clone(&shared);
        let joined = std::thread::spawn(move || {
            let _guard = holder.write().unwrap();
            panic!("poison the rwlock");
        })
        .join();
        assert!(joined.is_err());
        assert!(shared.is_poisoned());
        assert_eq!(*read(&shared), 7);
        *write(&shared) = 8;
        assert_eq!(*read(&shared), 8);
    }
}
