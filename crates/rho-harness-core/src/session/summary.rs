use super::cwd::forget_sessions_async;
use super::summary_scan::{ScannedSession, scan_session};
use crate::error::Result;
use crate::text::truncate_chars;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

const MAX_CONCURRENT_SCANS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
    pub turn_count: usize,
    pub preview: String,
}

fn jsonl_stem(path: &Path) -> Option<String> {
    if path.extension().is_some_and(|v| v == "jsonl") {
        path.file_stem().and_then(|v| v.to_str()).map(str::to_string)
    } else {
        None
    }
}

/// Sessions that fail to scan are left out of listings, with a warning naming the file.
fn summarize_path(path: &Path) -> Option<SessionSummary> {
    let stem = jsonl_stem(path)?;
    let (created_at, scanned) = match scan_session(path, &stem) {
        Ok(scan) => scan,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "skipping session that failed to load");
            return None;
        }
    };
    let last_modified: DateTime<Utc> = std::fs::metadata(path)
        .ok()?
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());
    Some(make_session_summary(stem, created_at, scanned, last_modified))
}

fn newest_first(mut summaries: Vec<SessionSummary>) -> Vec<SessionSummary> {
    summaries.sort_by_key(|b| std::cmp::Reverse(b.last_modified));
    summaries
}

pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(sessions_dir)? {
        summaries.extend(summarize_path(&entry?.path()));
    }
    Ok(newest_first(summaries))
}

async fn join_one(scans: &mut tokio::task::JoinSet<Option<SessionSummary>>, summaries: &mut Vec<SessionSummary>) {
    if let Some(Ok(Some(summary))) = scans.join_next().await {
        summaries.push(summary);
    }
}

/// Scans session files on the blocking pool, at most `MAX_CONCURRENT_SCANS` at a time.
async fn scan_paths_async(mut entries: tokio::fs::ReadDir) -> Result<Vec<SessionSummary>> {
    let mut scans = tokio::task::JoinSet::new();
    let mut summaries = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if jsonl_stem(&path).is_none() {
            continue;
        }
        if scans.len() >= MAX_CONCURRENT_SCANS {
            join_one(&mut scans, &mut summaries).await;
        }
        scans.spawn_blocking(move || summarize_path(&path));
    }
    while !scans.is_empty() {
        join_one(&mut scans, &mut summaries).await;
    }
    Ok(newest_first(summaries))
}

pub async fn list_session_summaries_async(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !tokio::fs::try_exists(sessions_dir).await? {
        return Ok(Vec::new());
    }
    let entries = tokio::fs::read_dir(sessions_dir).await?;
    scan_paths_async(entries).await
}

fn truncate_preview_text(preview: String) -> String {
    if truncate_chars(&preview, 50).1 {
        format!("{}...", truncate_chars(&preview, 47).0)
    } else {
        preview
    }
}

fn make_session_summary(
    session_id: String,
    created_at: DateTime<Utc>,
    scanned: ScannedSession,
    last_modified: DateTime<Utc>,
) -> SessionSummary {
    let preview = scanned
        .root_text
        .flatten()
        .unwrap_or_else(|| "Empty session".to_string());
    SessionSummary {
        session_id,
        name: scanned.name,
        created_at,
        last_modified,
        turn_count: scanned.node_count,
        preview: truncate_preview_text(preview),
    }
}

pub async fn delete_session_async(sessions_dir: &Path, session_id: &str) -> Result<()> {
    let file_path = sessions_dir.join(format!("{session_id}.jsonl"));
    if tokio::fs::try_exists(&file_path).await? {
        tokio::fs::remove_file(file_path).await?;
        forget_sessions_async(sessions_dir, BTreeSet::from([session_id.to_string()])).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChatMessage;
    use crate::session::SessionManager;

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
        let dir = crate::test_support::temp_dir();

        let session_a = SessionManager::new(&dir, None).unwrap();
        let sid_a = session_a.session_id.clone();
        session_a.set_session_name("Alpha Session").await.unwrap();
        let msgs_a = vec![
            ChatMessage::user("first question"),
            ChatMessage::assistant("first reply"),
        ];
        session_a.append_messages(&sid_a, msgs_a).await.unwrap();

        let session_b = SessionManager::new(&dir, None).unwrap();
        let sid_b = session_b.session_id.clone();
        let msgs_b = vec![
            ChatMessage::user("second session prompt"),
            ChatMessage::assistant("second reply"),
        ];
        session_b.append_messages(&sid_b, msgs_b).await.unwrap();

        let session_c = SessionManager::new(&dir, None).unwrap();
        let sid_c = session_c.session_id.clone();

        std::fs::write(dir.join("ignore.txt"), "not a session").unwrap();

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

        delete_session_async(&dir, &sid_a).await.unwrap();
        assert!(!dir.join(format!("{sid_a}.jsonl")).exists());
        delete_session_async(&dir, &sid_a).await.unwrap();

        delete_session_async(&dir, &sid_b).await.unwrap();
        assert!(!dir.join(format!("{sid_b}.jsonl")).exists());
        delete_session_async(&dir, &sid_b).await.unwrap();

        let remaining: Vec<String> = list_session_summaries_async(&dir)
            .await
            .unwrap()
            .into_iter()
            .map(|summary| summary.session_id)
            .collect();
        assert_eq!(remaining, vec![sid_c]);
    }

    #[tokio::test]
    async fn unloadable_session_file_is_left_out_of_both_listings() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new(&dir, None).unwrap();
        let sid = session.session_id.clone();
        std::fs::write(dir.join("broken.jsonl"), "not a session header\n").unwrap();

        let sync_ids: Vec<String> = list_session_summaries(&dir)
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        let async_ids: Vec<String> = list_session_summaries_async(&dir)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .collect();

        assert_eq!(sync_ids, vec![sid.clone()]);
        assert_eq!(async_ids, vec![sid]);
    }

    #[tokio::test]
    async fn summary_created_at_comes_from_the_session_header_not_the_file_mtime() {
        let dir = crate::test_support::temp_dir();
        let session = SessionManager::new(&dir, None).unwrap();
        let sid = session.session_id.clone();
        session
            .append_messages(&sid, vec![ChatMessage::user("q"), ChatMessage::assistant("a")])
            .await
            .unwrap();
        let path = dir.join(format!("{sid}.jsonl"));
        let raw = std::fs::read_to_string(&path).unwrap();
        let (header, rest) = raw.split_once('\n').unwrap();
        let mut header: serde_json::Value = serde_json::from_str(header).unwrap();
        header["created_at"] = "2020-01-02T03:04:05Z".into();
        std::fs::write(&path, format!("{header}\n{rest}")).unwrap();

        let summary = list_session_summaries(&dir).unwrap().remove(0);

        assert_eq!(summary.created_at.to_rfc3339(), "2020-01-02T03:04:05+00:00");
        assert!(summary.last_modified > summary.created_at);
    }
}
