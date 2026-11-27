use tokio::sync::Notify;

/// Returns what `ready` yields once it yields something, re-checking each time `notify` wakes its
/// waiters. `notify_waiters` stores no permit, so the waiter is registered before `ready` runs.
pub async fn until<T>(notify: &Notify, mut ready: impl FnMut() -> Option<T>) -> T {
    loop {
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if let Some(value) = ready() {
            return value;
        }
        notified.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn a_condition_set_before_the_wake_is_never_missed() {
        let notify = Arc::new(Notify::new());
        let flag = Arc::new(AtomicBool::new(false));
        let waiter = tokio::spawn({
            let (notify, flag) = (notify.clone(), flag.clone());
            async move { until(&notify, || flag.load(Ordering::Acquire).then_some(7)).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        flag.store(true, Ordering::Release);
        notify.notify_waiters();

        assert_eq!(waiter.await.unwrap(), 7);
        assert_eq!(until(&notify, || Some("ready")).await, "ready");
    }
}
