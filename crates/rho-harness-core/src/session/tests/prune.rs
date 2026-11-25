use super::super::prune::{prune_empty_sessions_async, prune_expired_sessions_async};
use super::temp_dir;
use crate::session::SessionManager;
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

fn seeded_dir(sessions: &[(&str, u64, bool)]) -> crate::test_support::TestDir {
    let dir = temp_dir();
    for (id, age_days, is_named) in sessions {
        create_test_session_file(&dir, id, *age_days, *is_named);
    }
    dir
}

fn assert_sessions(dir: &Path, kept: &[&str], removed: &[&str]) {
    for id in kept {
        assert!(dir.join(format!("{id}.jsonl")).exists(), "{id} should be kept");
    }
    for id in removed {
        assert!(!dir.join(format!("{id}.jsonl")).exists(), "{id} should be pruned");
    }
}

type SeedList<'a> = &'a [(&'a str, u64, bool)];

struct ExpiryCase<'a> {
    name: &'a str,
    sessions: SeedList<'a>,
    retention_days: u32,
    expected_pruned: usize,
    kept: &'a [&'a str],
    removed: &'a [&'a str],
}

#[tokio::test]
async fn expired_session_pruning_follows_retention_rules() {
    let cases = [
        ExpiryCase {
            name: "prunes expired unnamed sessions",
            sessions: &[("old_session", 6, false), ("recent_session", 2, false)],
            retention_days: 5,
            expected_pruned: 1,
            kept: &["recent_session"],
            removed: &["old_session"],
        },
        ExpiryCase {
            name: "preserves the active session even if old",
            sessions: &[("active_session", 10, false)],
            retention_days: 5,
            expected_pruned: 0,
            kept: &["active_session"],
            removed: &[],
        },
        ExpiryCase {
            name: "preserves named sessions even if old",
            sessions: &[("named_old_session", 10, true), ("unnamed_old_session", 10, false)],
            retention_days: 5,
            expected_pruned: 1,
            kept: &["named_old_session"],
            removed: &["unnamed_old_session"],
        },
        ExpiryCase {
            name: "is disabled when retention days is zero",
            sessions: &[("old_session", 30, false)],
            retention_days: 0,
            expected_pruned: 0,
            kept: &["old_session"],
            removed: &[],
        },
        ExpiryCase {
            name: "mixed old, recent and named sessions",
            sessions: &[
                ("old_session_1", 7, false),
                ("recent_session_1", 1, false),
                ("named_old_session_1", 10, true),
            ],
            retention_days: 5,
            expected_pruned: 1,
            kept: &["recent_session_1", "named_old_session_1"],
            removed: &["old_session_1"],
        },
    ];
    for case in cases {
        let dir = seeded_dir(case.sessions);
        let pruned = prune_expired_sessions_async(&dir, "active_session", case.retention_days)
            .await
            .unwrap();
        assert_eq!(pruned, case.expected_pruned, "{}", case.name);
        assert_sessions(&dir, case.kept, case.removed);
    }
}

#[tokio::test]
async fn empty_session_pruning_keeps_active_and_named_sessions() {
    let seeds: SeedList = &[
        ("empty_session", 0, false),
        ("active_empty", 0, false),
        ("named_empty", 0, true),
    ];
    let kept = ["active_empty", "named_empty"];

    let dir = seeded_dir(seeds);
    assert_eq!(prune_empty_sessions_async(&dir, Some("active_empty")).await.unwrap(), 1);
    assert_sessions(&dir, &kept, &["empty_session"]);
}

#[tokio::test]
async fn pruning_skips_sessions_held_open_by_a_manager() {
    let dir = crate::test_support::temp_dir();
    let holder = SessionManager::new_async(&dir, None).await.unwrap();
    let held_path = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .unwrap();
    assert!(std::fs::metadata(&held_path).unwrap().len() <= 300);

    assert_eq!(prune_empty_sessions_async(&dir, None).await.unwrap(), 0);
    assert!(held_path.exists());

    drop(holder);
    assert_eq!(prune_empty_sessions_async(&dir, None).await.unwrap(), 1);
    assert!(!held_path.exists());
}

fn file_of_exact_size(dir: &Path, name: &str, size: usize, marker: &str) -> std::path::PathBuf {
    let path = dir.join(format!("{name}.jsonl"));
    let mut content = format!("{{\"type\":\"header\"}}\n{marker}");
    assert!(content.len() <= size);
    content.push_str(&" ".repeat(size - content.len()));
    std::fs::write(&path, content).unwrap();
    path
}

#[tokio::test]
async fn empty_pruning_depends_on_size_and_on_tree_or_name_records() {
    let dir = temp_dir();
    let cases = [
        ("at_limit", 300, "", false),
        ("over_limit", 301, "", true),
        ("small_with_tree_node", 100, "{\"type\":\"tree_node\"}", true),
        ("small_with_name", 100, "{\"type\":\"session_named\"}", true),
        ("small_with_other_record", 100, "{\"type\":\"audit_event\"}", false),
    ];
    for (name, size, marker, _) in cases {
        file_of_exact_size(&dir, name, size, marker);
    }

    let removed = prune_empty_sessions_async(&dir, None).await.unwrap();

    let expected_kept: Vec<&str> = cases.iter().filter(|c| c.3).map(|c| c.0).collect();
    assert_eq!(removed, cases.len() - expected_kept.len());
    for (name, _, _, kept) in cases {
        assert_eq!(dir.join(format!("{name}.jsonl")).exists(), kept, "{name}");
    }
}

#[tokio::test]
async fn pruning_never_touches_the_active_session_or_non_session_files() {
    let dir = temp_dir();
    let active = file_of_exact_size(&dir, "active", 50, "");
    std::fs::write(dir.join("notes.txt"), "tiny").unwrap();

    let removed = prune_empty_sessions_async(&dir, Some("active")).await.unwrap();

    assert_eq!(removed, 0);
    assert!(active.exists() && dir.join("notes.txt").exists());
}

#[tokio::test]
async fn spawn_auto_prune_removes_only_expired_inactive_sessions_in_the_background() {
    let dir = temp_dir();
    let active = SessionManager::new_async(&dir, None).await.unwrap();
    create_test_session_file(&dir, "old", 30, false);
    create_test_session_file(&dir, "recent", 1, false);

    active.spawn_auto_prune(7).await.unwrap();

    assert!(!dir.join("old.jsonl").exists(), "expired session was pruned");
    assert!(dir.join("recent.jsonl").exists());
    assert!(active.file_path.exists());
}
