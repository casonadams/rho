use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn counting_token() -> (Arc<AtomicUsize>, ActivityToken) {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let token = activity_token(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });
    (counter, token)
}

#[test]
fn dropping_token_invokes_finisher() {
    let (counter, token) = counting_token();

    assert_eq!(counter.load(Ordering::SeqCst), 0);
    drop(token);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_and_clear_prevents_drop_invocation() {
    let (counter, token) = counting_token();

    token.finish_and_clear();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_cloned_token_invokes_finisher_only_on_last_drop() {
    let (counter, token1) = counting_token();
    let token2 = token1.clone();

    drop(token1);
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    drop(token2);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn finish_and_clear_on_clone_prevents_last_drop_invocation() {
    let (counter, token1) = counting_token();
    let token2 = token1.clone();

    token1.finish_and_clear();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    drop(token2);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn disarm_prevents_finisher_on_drop() {
    let (counter, token) = counting_token();

    token.disarm();
    drop(token);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}
