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
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
        {
            ids.push(stem.to_string());
        }
    }
    ids.sort();
    ids.reverse();
    Ok(ids)
}

fn jsonl_stem(path: &Path) -> Option<String> {
    if path.extension().and_then(|v| v.to_str()) == Some("jsonl") {
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

pub fn list_session_summaries(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
            && let Ok(state) = load_file(&path, stem)
        {
            let metadata = std::fs::metadata(&path)?;
            let last_modified: DateTime<Utc> = metadata
                .modified()
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(|_| Utc::now());
            summaries.push(make_session_summary(stem, state, last_modified));
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
