use super::{SessionManager, temp_dir};
use std::io::Write;

#[cfg(unix)]
#[test]
fn session_storage_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(&store.file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let id = store.session_id.clone();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&store.file_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    drop(store);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(&resumed.file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let bad_path = dir.join("bad.jsonl");
    std::fs::write(&bad_path, "{}\n").unwrap();
    std::fs::set_permissions(&bad_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(SessionManager::new(&dir, Some("bad")).is_err());
    assert_eq!(
        std::fs::metadata(&bad_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
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
fn malformed_committed_records_fail_but_incomplete_tail_is_ignored() {
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

    let bad_dir = temp_dir();
    let bad = SessionManager::new(&bad_dir, None).unwrap();
    let bad_id = bad.session_id.clone();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&bad.file_path)
        .unwrap()
        .write_all(b"{malformed}\n")
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&bad.file_path)
        .unwrap()
        .write_all(b"{\"record_type\":\"canonical_reset\",\"sequence\":1,\"session_id\":\"ignored\",\"timestamp\":\"2025-01-01T00:00:00Z\"}\n")
        .unwrap();
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
