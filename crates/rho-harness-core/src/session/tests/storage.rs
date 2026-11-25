use super::{SessionManager, temp_dir};
use crate::model::ChatMessage;
use std::io::Write;

#[cfg(unix)]
fn assert_mode(path: &std::path::Path, expected: u32) {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, expected);
}

#[cfg(unix)]
fn write_with_mode(path: &std::path::Path, contents: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn session_storage_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    assert_mode(&dir, 0o700);
    assert_mode(&store.file_path, 0o600);

    let id = store.session_id.clone();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&store.file_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    drop(store);

    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_mode(&dir, 0o700);
    assert_mode(&resumed.file_path, 0o600);
}

#[cfg(unix)]
#[test]
fn session_storage_protects_bad_file_permissions() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let bad_path = dir.join("bad.jsonl");
    write_with_mode(&bad_path, "{}\n", 0o644);
    assert!(SessionManager::new(&dir, Some("bad")).is_err());
    assert_mode(&bad_path, 0o600);
}

#[test]
fn resume_requires_supported_versioned_header() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for (id, body, expected) in [
        ("legacy", r#"{"id":"old"}\n"#, "legacy session"),
        (
            "missing",
            r#"{"record_type":"header","session_id":"missing"}\n"#,
            "mandatory version",
        ),
        (
            "wrong",
            r#"{"record_type":"header","version":1,"session_id":"wrong","created_at":"2025-01-01T00:00:00Z"}\n"#,
            "unsupported session version",
        ),
        (
            "future",
            r#"{"record_type":"header","version":3,"session_id":"future","created_at":"2025-01-01T00:00:00Z"}\n"#,
            "unsupported session version",
        ),
        (
            "mismatch",
            r#"{"record_type":"header","version":2,"session_id":"someone-else","created_at":"2025-01-01T00:00:00Z"}\n"#,
            "does not match its file name",
        ),
    ] {
        std::fs::write(dir.join(format!("{id}.jsonl")), body.replace("\\n", "\n")).unwrap();
        let error = SessionManager::new(&dir, Some(id)).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn missing_and_unknown_sessions_fail_clearly() {
    let dir = temp_dir();
    let missing = SessionManager::new(&dir, Some("unknown")).unwrap_err().to_string();
    assert!(missing.contains("unknown session id"));
    std::fs::write(dir.join("empty.jsonl"), "").unwrap();
    let empty = SessionManager::new(&dir, Some("empty")).unwrap_err().to_string();
    assert!(empty.contains("mandatory version header"));
}

#[test]
fn incomplete_tail_record_is_ignored() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&store.file_path)
        .unwrap()
        .write_all(b"{interrupted")
        .unwrap();
    assert!(SessionManager::new(&dir, Some(&id)).is_ok());
}

#[test]
fn malformed_committed_records_fail() {
    let bad_dir = temp_dir();
    let bad = SessionManager::new(&bad_dir, None).unwrap();
    let bad_id = bad.session_id.clone();
    let mut file = std::fs::OpenOptions::new().append(true).open(&bad.file_path).unwrap();
    file.write_all(b"{malformed}\n").unwrap();
    file.write_all(b"{\"record_type\":\"canonical_reset\",\"sequence\":1,\"session_id\":\"ignored\",\"timestamp\":\"2025-01-01T00:00:00Z\"}\n").unwrap();
    let error = SessionManager::new(&bad_dir, Some(&bad_id)).unwrap_err().to_string();
    assert!(error.contains("malformed committed record"));
}

#[test]
fn last_session_for_cwd_tracks_and_resolves() {
    let sessions_dir = temp_dir();
    let project_a = temp_dir();
    let project_b = temp_dir();
    std::fs::create_dir_all(&sessions_dir).unwrap();
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();

    SessionManager::record_session_for_cwd(&sessions_dir, &project_a, "sess-a1").unwrap();
    SessionManager::record_session_for_cwd(&sessions_dir, &project_b, "sess-b1").unwrap();
    SessionManager::record_session_for_cwd(&sessions_dir, &project_a, "sess-a2").unwrap();

    assert_eq!(
        SessionManager::last_session_for_cwd(&sessions_dir, &project_a)
            .unwrap()
            .as_deref(),
        Some("sess-a2")
    );
    assert_eq!(
        SessionManager::last_session_for_cwd(&sessions_dir, &project_b)
            .unwrap()
            .as_deref(),
        Some("sess-b1")
    );
}

#[tokio::test]
async fn last_session_for_cwd_async_resolves_missing_directories_by_raw_path() {
    let sessions_dir = temp_dir();
    let project = temp_dir();
    SessionManager::record_session_for_cwd(&sessions_dir, &project, "sess-async").unwrap();

    let asynchronous = SessionManager::last_session_for_cwd_async(&sessions_dir, &project).await;
    assert_eq!(asynchronous.unwrap().as_deref(), Some("sess-async"));
    let other = temp_dir();
    assert_eq!(
        SessionManager::last_session_for_cwd_async(&sessions_dir, &other)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn cwd_for_session_tracks_latest_relocated_workspace() {
    let sessions_dir = temp_dir();
    let repo_dir = temp_dir();
    let worktree_dir = temp_dir();
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::create_dir_all(&worktree_dir).unwrap();

    SessionManager::record_session_for_cwd(&sessions_dir, &repo_dir, "sess-1").unwrap();
    assert_eq!(
        SessionManager::cwd_for_session(&sessions_dir, "sess-1").unwrap(),
        Some(std::fs::canonicalize(&repo_dir).unwrap())
    );

    // Relocating the session to a worktree updates session_id -> cwd.
    SessionManager::record_session_for_cwd(&sessions_dir, &worktree_dir, "sess-1").unwrap();
    assert_eq!(
        SessionManager::cwd_for_session_async(&sessions_dir, "sess-1")
            .await
            .unwrap(),
        Some(std::fs::canonicalize(&worktree_dir).unwrap())
    );
    assert_eq!(SessionManager::cwd_for_session(&sessions_dir, "unknown").unwrap(), None);
}

type IndexFixture = fn(&std::path::Path);

#[tokio::test]
async fn last_session_lookup_reports_unreadable_or_corrupt_index() {
    let cases: &[(&str, IndexFixture)] = &[
        ("corrupt_json", |index| std::fs::write(index, "{not json").unwrap()),
        ("index_is_directory", |index| std::fs::create_dir_all(index).unwrap()),
    ];
    for (name, prepare) in cases {
        let sessions_dir = temp_dir();
        std::fs::create_dir_all(&sessions_dir).unwrap();
        prepare(&sessions_dir.join(".last_sessions.json"));
        let project = temp_dir();
        assert!(
            SessionManager::last_session_for_cwd(&sessions_dir, &project).is_err(),
            "{name}"
        );
        assert!(
            SessionManager::last_session_for_cwd_async(&sessions_dir, &project)
                .await
                .is_err(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn every_write_path_rejects_secrets_without_echoing_or_persisting_them() {
    use crate::a2a::{Task, TaskState};
    use crate::session::SessionEventKind;

    const SECRET: &str = "credential-sentinel-9f3a";
    let dir = temp_dir();
    let store = SessionManager::new_with_secrets(&dir, None, vec![SECRET.to_string()]).unwrap();
    let id = store.session_id.clone();
    let leaked = || ChatMessage::user(format!("token {SECRET}"));
    let mut task = Task::new("t1", "c1", None);
    task.set_status(TaskState::Working, None);

    let history = store.append_messages(&id, vec![leaked()]).await;
    let event = store
        .append_event(SessionEventKind::UserMessage, serde_json::json!({ "text": SECRET }))
        .await;
    let subagent = store.record_subagent_task(&task, vec![leaked()]).await;
    let replaced = store.replace_subagent_transcript(&task, vec![leaked()]).await;
    let summary = store.append_branch_summary(&format!("notes {SECRET}"), "leaf").await;
    let extended = store.extend_checkpoint(vec![leaked()]).await;
    store.save_checkpoint(vec![ChatMessage::user("clean")]).await.unwrap();
    let promoted = store.promote_checkpoint(vec![leaked()]).await;

    let outcomes = [
        ("append_messages", history),
        ("append_event", event),
        ("record_subagent_task", subagent),
        ("replace_subagent_transcript", replaced),
        ("append_branch_summary", summary),
        ("extend_checkpoint", extended),
        ("promote_checkpoint", promoted),
    ];
    for (api, outcome) in outcomes {
        let error = outcome.expect_err(api).to_string();
        assert!(!error.contains(SECRET), "{api} echoed the secret: {error}");
    }
    let on_disk = std::fs::read_to_string(&store.file_path).unwrap();
    assert!(!on_disk.contains(SECRET));
    assert!(store.subagent_contexts().await.is_empty());
    assert_eq!(store.load_checkpoint().await.unwrap().unwrap().len(), 1);
}

#[tokio::test]
async fn two_managers_on_a_live_session_share_one_history_and_stay_loadable() {
    let dir = temp_dir();
    let first = SessionManager::new_async(&dir, None).await.unwrap();
    let id = first.session_id.clone();
    first
        .append_messages(&id, vec![ChatMessage::user("one"), ChatMessage::assistant("1")])
        .await
        .unwrap();

    let second = SessionManager::new_async(&dir, Some(&id)).await.unwrap();
    first
        .append_messages(&id, vec![ChatMessage::user("two"), ChatMessage::assistant("2")])
        .await
        .unwrap();
    second
        .append_messages(&id, vec![ChatMessage::user("three"), ChatMessage::assistant("3")])
        .await
        .unwrap();

    let texts = |messages: &[ChatMessage]| {
        messages
            .iter()
            .filter_map(|m| m.text().map(str::to_string))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts(&first.load_messages().await.unwrap()),
        ["one", "1", "two", "2", "three", "3"]
    );
    assert_eq!(
        texts(&second.load_messages().await.unwrap()),
        texts(&first.load_messages().await.unwrap())
    );

    drop((first, second));
    let reloaded = SessionManager::new_async(&dir, Some(&id)).await.unwrap();
    assert_eq!(texts(&reloaded.load_messages().await.unwrap()).len(), 6);
}
