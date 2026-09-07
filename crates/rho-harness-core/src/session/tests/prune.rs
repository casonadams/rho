use super::super::prune::{prune_expired_sessions, prune_expired_sessions_async};
use super::temp_dir;
use std::fs::File;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn create_test_session_file(dir: &Path, session_id: &str, age_days: u64, is_named: bool) {
    let path = dir.join(format!("{session_id}.jsonl"));
    let mut content = format!(
        "{{\"record_type\":\"header\",\"version\":1,\"session_id\":\"{session_id}\",\"created_at\":\"2026-08-01T00:00:00Z\"}}\n"
    );
    if is_named {
        content.push_str(&format!(
            "{{\"record_type\":\"session_named\",\"sequence\":1,\"session_id\":\"{session_id}\",\"timestamp\":\"2026-08-01T00:00:01Z\",\"name\":\"Saved Session\"}}\n"
        ));
    }
    std::fs::write(&path, content).unwrap();

    let file = File::open(&path).unwrap();
    let mtime = SystemTime::now() - Duration::from_secs(age_days * 86_400 + 3600);
    let times = std::fs::FileTimes::new().set_modified(mtime);
    file.set_times(times).unwrap();
}

#[test]
fn prunes_expired_unnamed_sessions() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();

    create_test_session_file(&dir, "old_session", 6, false);
    create_test_session_file(&dir, "recent_session", 2, false);

    let pruned = prune_expired_sessions(&dir, "active_session", 5).unwrap();
    assert_eq!(pruned, 1);
    assert!(!dir.join("old_session.jsonl").exists());
    assert!(dir.join("recent_session.jsonl").exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn preserves_active_session_even_if_old() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();

    create_test_session_file(&dir, "active_session", 10, false);

    let pruned = prune_expired_sessions(&dir, "active_session", 5).unwrap();
    assert_eq!(pruned, 0);
    assert!(dir.join("active_session.jsonl").exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn preserves_named_sessions_even_if_old() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();

    create_test_session_file(&dir, "named_old_session", 10, true);
    create_test_session_file(&dir, "unnamed_old_session", 10, false);

    let pruned = prune_expired_sessions(&dir, "active_session", 5).unwrap();
    assert_eq!(pruned, 1);
    assert!(dir.join("named_old_session.jsonl").exists());
    assert!(!dir.join("unnamed_old_session.jsonl").exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn disabled_when_retention_days_is_zero() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();

    create_test_session_file(&dir, "old_session", 30, false);

    let pruned = prune_expired_sessions(&dir, "active_session", 0).unwrap();
    assert_eq!(pruned, 0);
    assert!(dir.join("old_session.jsonl").exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn async_prunes_expired_unnamed_sessions() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();

    create_test_session_file(&dir, "old_session_1", 7, false);
    create_test_session_file(&dir, "recent_session_1", 1, false);
    create_test_session_file(&dir, "named_old_session_1", 10, true);

    let pruned = prune_expired_sessions_async(&dir, "active_session", 5).await.unwrap();
    assert_eq!(pruned, 1);
    assert!(!dir.join("old_session_1.jsonl").exists());
    assert!(dir.join("recent_session_1.jsonl").exists());
    assert!(dir.join("named_old_session_1.jsonl").exists());

    let _ = std::fs::remove_dir_all(dir);
}
