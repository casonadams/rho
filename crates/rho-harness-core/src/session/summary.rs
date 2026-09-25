use super::format::{StoreState, load_file, load_file_async};
use crate::error::Result;
use chrono::{DateTime, Utc};
use rig::message::Message;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
    pub turn_count: usize,
    pub preview: String,
}

pub fn list_sessions(sessions_dir: &Path) -> Result<Vec<String>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(sessions_dir)? {
        if let Some(stem) = jsonl_stem(&entry?.path()) {
            ids.push(stem);
        }
    }
    ids.sort();
    ids.reverse();
    Ok(ids)
}

fn jsonl_stem(path: &Path) -> Option<String> {
    if path.extension().is_some_and(|v| v == "jsonl") {
        path.file_stem().and_then(|v| v.to_str()).map(str::to_string)
    } else {
        None
    }
}

async fn drain_session_ids_async(mut entries: tokio::fs::ReadDir) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if let Some(stem) = jsonl_stem(&entry.path()) {
            ids.push(stem);
        }
    }
    ids.sort();
    ids.reverse();
    Ok(ids)
}

pub async fn list_sessions_async(sessions_dir: &Path) -> Result<Vec<String>> {
    if !tokio::fs::try_exists(sessions_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let entries = tokio::fs::read_dir(sessions_dir).await?;
    drain_session_ids_async(entries).await
}

fn summarize_entry(entry: &std::fs::DirEntry) -> Option<SessionSummary> {
    let path = entry.path();
    let stem = jsonl_stem(&path)?;
    let state = load_file(&path, &stem).ok()?;
    let metadata = entry.metadata().ok()?;
    let last_modified: DateTime<Utc> = metadata
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());
    Some(make_session_summary(&stem, state, last_modified))
}

pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(sessions_dir)? {
        if let Some(summary) = summarize_entry(&entry?) {
            summaries.push(summary);
        }
    }
    summaries.sort_by_key(|b| std::cmp::Reverse(b.last_modified));
    Ok(summaries)
}

async fn summarize_entry_async(entry: &tokio::fs::DirEntry) -> Option<SessionSummary> {
    let path = entry.path();
    let stem = jsonl_stem(&path)?;
    let state = load_file_async(&path, &stem).await.ok()?;
    let metadata = entry.metadata().await.ok()?;
    let last_modified: DateTime<Utc> = metadata
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());
    Some(make_session_summary(&stem, state, last_modified))
}

async fn collect_summary_set(mut entries: tokio::fs::ReadDir) -> Result<tokio::task::JoinSet<Option<SessionSummary>>> {
    let mut set = tokio::task::JoinSet::new();
    while let Some(entry) = entries.next_entry().await? {
        set.spawn(async move { summarize_entry_async(&entry).await });
    }
    Ok(set)
}

async fn drain_summaries_async(entries: tokio::fs::ReadDir) -> Result<Vec<SessionSummary>> {
    let mut set = collect_summary_set(entries).await?;
    let mut summaries = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(Some(summary)) = res {
            summaries.push(summary);
        }
    }
    summaries.sort_by_key(|b| std::cmp::Reverse(b.last_modified));
    Ok(summaries)
}

pub async fn list_session_summaries_async(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !tokio::fs::try_exists(sessions_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let entries = tokio::fs::read_dir(sessions_dir).await?;
    drain_summaries_async(entries).await
}

fn extract_root_preview(state: &StoreState) -> String {
    state
        .tree
        .root_nodes()
        .first()
        .and_then(|n| {
            n.messages.iter().find_map(|m| match m {
                Message::User { content } => content.first().map(|c| match c {
                    rig::message::UserContent::Text(t) => t.text.clone(),
                    _ => String::new(),
                }),
                _ => None,
            })
        })
        .unwrap_or_else(|| "Empty session".to_string())
}

fn truncate_preview_text(preview: String) -> String {
    if preview.chars().count() > 50 {
        format!("{}...", preview.chars().take(47).collect::<String>())
    } else {
        preview
    }
}

fn make_session_summary(stem: &str, state: StoreState, last_modified: DateTime<Utc>) -> SessionSummary {
    let turn_count = state.tree.len();
    let preview = truncate_preview_text(extract_root_preview(&state));
    SessionSummary {
        session_id: stem.to_string(),
        name: state.tree.session_name,
        created_at: last_modified,
        last_modified,
        turn_count,
        preview,
    }
}

pub fn delete_session(sessions_dir: &Path, session_id: &str) -> Result<()> {
    let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
    if file_path.exists() {
        std::fs::remove_file(file_path)?;
    }
    Ok(())
}

pub async fn delete_session_async(sessions_dir: &Path, session_id: &str) -> Result<()> {
    let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
    if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
        tokio::fs::remove_file(file_path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionManager;
    use rig::message::Message;

    fn temp_test_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("session_summary_test_{label}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_jsonl_stem() {
        assert_eq!(jsonl_stem(Path::new("session.jsonl")), Some("session".to_string()));
        assert_eq!(
            jsonl_stem(Path::new("/a/b/c/sess-123.jsonl")),
            Some("sess-123".to_string())
        );
        assert_eq!(jsonl_stem(Path::new("session.txt")), None);
        assert_eq!(jsonl_stem(Path::new("no_extension")), None);
        assert_eq!(jsonl_stem(Path::new("")), None);
    }

    #[test]
    fn test_truncate_preview_text() {
        assert_eq!(truncate_preview_text("short preview".to_string()), "short preview");
        let exact_50 = "a".repeat(50);
        assert_eq!(truncate_preview_text(exact_50.clone()), exact_50);
        let long_51 = "a".repeat(51);
        let expected = format!("{}...", "a".repeat(47));
        assert_eq!(truncate_preview_text(long_51), expected);
    }

    #[test]
    fn test_list_sessions_nonexistent_dir() {
        let non_existent = Path::new("/tmp/does_not_exist_session_dir_xyz123");
        assert_eq!(list_sessions(non_existent).unwrap(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn test_list_sessions_async_nonexistent_dir() {
        let non_existent = Path::new("/tmp/does_not_exist_session_dir_xyz123");
        assert_eq!(list_sessions_async(non_existent).await.unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_list_session_summaries_nonexistent_dir() {
        let non_existent = Path::new("/tmp/does_not_exist_session_dir_xyz123");
        assert_eq!(
            list_session_summaries(non_existent).unwrap(),
            Vec::<SessionSummary>::new()
        );
    }

    #[tokio::test]
    async fn test_list_session_summaries_async_nonexistent_dir() {
        let non_existent = Path::new("/tmp/does_not_exist_session_dir_xyz123");
        assert_eq!(
            list_session_summaries_async(non_existent).await.unwrap(),
            Vec::<SessionSummary>::new()
        );
    }

    #[tokio::test]
    async fn test_list_and_summarize_sessions() {
        let dir = temp_test_dir("list_and_summarize");

        let session_a = SessionManager::new(&dir, None).unwrap();
        let sid_a = session_a.session_id.clone();
        session_a.set_session_name("Alpha Session").await.unwrap();
        session_a
            .append_messages(
                &sid_a,
                vec![Message::user("first question"), Message::assistant("first reply")],
            )
            .await
            .unwrap();

        let session_b = SessionManager::new(&dir, None).unwrap();
        let sid_b = session_b.session_id.clone();
        session_b
            .append_messages(
                &sid_b,
                vec![
                    Message::user("second session prompt"),
                    Message::assistant("second reply"),
                ],
            )
            .await
            .unwrap();

        let session_c = SessionManager::new(&dir, None).unwrap();
        let sid_c = session_c.session_id.clone();

        std::fs::write(dir.join("ignore.txt"), "not a session").unwrap();

        let mut expected_ids = vec![sid_a.clone(), sid_b.clone(), sid_c.clone()];
        expected_ids.sort();
        expected_ids.reverse();

        let listed = list_sessions(&dir).unwrap();
        assert_eq!(listed, expected_ids);

        let listed_async = list_sessions_async(&dir).await.unwrap();
        assert_eq!(listed_async, expected_ids);

        let summaries = list_session_summaries(&dir).unwrap();
        assert_eq!(summaries.len(), 3);
        let alpha_summary = summaries.iter().find(|s| s.session_id == sid_a).unwrap();
        assert_eq!(alpha_summary.name, Some("Alpha Session".to_string()));
        assert_eq!(alpha_summary.preview, "first question");
        assert_eq!(alpha_summary.turn_count, 1);

        let empty_summary = summaries.iter().find(|s| s.session_id == sid_c).unwrap();
        assert_eq!(empty_summary.preview, "Empty session");
        assert_eq!(empty_summary.turn_count, 0);

        let summaries_async = list_session_summaries_async(&dir).await.unwrap();
        assert_eq!(summaries_async.len(), 3);
        let alpha_async = summaries_async.iter().find(|s| s.session_id == sid_a).unwrap();
        assert_eq!(alpha_async.name, Some("Alpha Session".to_string()));
        assert_eq!(alpha_async.preview, "first question");

        delete_session(&dir, &sid_a).unwrap();
        assert!(!dir.join(format!("{sid_a}.jsonl")).exists());
        delete_session(&dir, &sid_a).unwrap();

        delete_session_async(&dir, &sid_b).await.unwrap();
        assert!(!dir.join(format!("{sid_b}.jsonl")).exists());
        delete_session_async(&dir, &sid_b).await.unwrap();

        let remaining = list_sessions(&dir).unwrap();
        assert_eq!(remaining, vec![sid_c]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
