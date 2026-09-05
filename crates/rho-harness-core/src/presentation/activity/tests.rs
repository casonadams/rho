use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn dropping_token_invokes_finisher() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let token = activity_token(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    assert_eq!(counter.load(Ordering::SeqCst), 0);
    drop(token);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_and_clear_prevents_drop_invocation() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let token = activity_token(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    token.finish_and_clear();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_cloned_token_invokes_finisher_only_on_last_drop() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let token1 = activity_token(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });
    let token2 = token1.clone();

    drop(token1);
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    drop(token2);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_and_clear_on_clone_prevents_last_drop_invocation() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let token1 = activity_token(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });
    let token2 = token1.clone();

    token1.finish_and_clear();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    drop(token2);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
