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

pub async fn list_sessions_async(sessions_dir: &Path) -> Result<Vec<String>> {
    if !tokio::fs::try_exists(sessions_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    let mut entries = tokio::fs::read_dir(sessions_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
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

pub async fn list_session_summaries_async(sessions_dir: &Path) -> Result<Vec<SessionSummary>> {
    if !tokio::fs::try_exists(sessions_dir).await.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    let mut entries = tokio::fs::read_dir(sessions_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
            && let Ok(state) = load_file_async(&path, stem).await
        {
            let metadata = entry.metadata().await?;
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

fn make_session_summary(stem: &str, state: StoreState, last_modified: DateTime<Utc>) -> SessionSummary {
    let turn_count = state.tree.len();
    let preview = state
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
        .unwrap_or_else(|| "Empty session".to_string());
    let preview_truncated = if preview.chars().count() > 50 {
        format!("{}...", preview.chars().take(47).collect::<String>())
    } else {
        preview
    };
    SessionSummary {
        session_id: stem.to_string(),
        name: state.tree.session_name,
        created_at: last_modified,
        last_modified,
        turn_count,
        preview: preview_truncated,
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
