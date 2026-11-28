use super::{SessionEventKind, SessionManager, temp_dir};
use crate::error::Result;
use crate::session::format::inject_write_hook;
use serde_json::json;
use std::future::Future;

/// Runs `op` in a spawned task and cancels that task once its write is in flight. The write
/// itself is released afterwards and must still complete.
async fn abort_caller_mid_write<F, Fut>(manager: &SessionManager, op: F)
where
    F: FnOnce(SessionManager) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    inject_write_hook(&manager.file_path, move || {
        let _ = reached_tx.send(());
        let _ = release_rx.blocking_recv();
        Ok(())
    });
    let caller = tokio::spawn(op(manager.clone()));
    reached_rx.await.unwrap();
    caller.abort();
    release_tx.send(()).unwrap();
    assert!(caller.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn aborting_an_audit_append_mid_write_keeps_later_appends_loadable() {
    let dir = temp_dir();
    let manager = SessionManager::new_async(&dir, None).await.unwrap();
    let session_id = manager.session_id.clone();
    let path = manager.file_path.clone();

    abort_caller_mid_write(&manager, |m| async move {
        m.append_event(SessionEventKind::UserMessage, json!({"n": 1})).await
    })
    .await;
    manager
        .append_event(SessionEventKind::UserMessage, json!({"n": 2}))
        .await
        .unwrap();
    drop(manager);

    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
    let reloaded = SessionManager::new_async(&dir, Some(&session_id)).await.unwrap();
    assert_eq!(reloaded.load_events().await.unwrap().len(), 2);
}

#[tokio::test]
async fn aborting_a_durable_append_mid_write_keeps_later_appends_loadable() {
    let dir = temp_dir();
    let manager = SessionManager::new_async(&dir, None).await.unwrap();
    let session_id = manager.session_id.clone();

    abort_caller_mid_write(&manager, |m| async move { m.set_session_name("First").await }).await;
    manager.set_session_name("Second").await.unwrap();
    drop(manager);

    let reloaded = SessionManager::new_async(&dir, Some(&session_id)).await.unwrap();
    assert_eq!(
        reloaded.load_tree().await.unwrap().session_name.as_deref(),
        Some("Second")
    );
}

#[tokio::test]
async fn failed_write_after_bytes_reached_disk_keeps_memory_matching_the_file() {
    let dir = temp_dir();
    let manager = SessionManager::new_async(&dir, None).await.unwrap();
    let session_id = manager.session_id.clone();
    let path = manager.file_path.clone();

    inject_write_hook(&path, || Err(std::io::Error::other("injected fsync failure")));
    assert!(manager.set_session_name("Failed").await.is_err());
    manager.set_session_name("Kept").await.unwrap();
    let in_memory = manager.load_tree().await.unwrap().session_name;
    drop(manager);

    let reloaded = SessionManager::new_async(&dir, Some(&session_id)).await.unwrap();
    assert_eq!(reloaded.load_tree().await.unwrap().session_name, in_memory);
    assert_eq!(in_memory.as_deref(), Some("Kept"));
}
