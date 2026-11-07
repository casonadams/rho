use super::super::prune::{prune_empty_sessions_async, prune_expired_sessions_async};
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

fn seeded_dir(sessions: &[(&str, u64, bool)]) -> std::path::PathBuf {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
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
    let _ = std::fs::remove_dir_all(dir);
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
