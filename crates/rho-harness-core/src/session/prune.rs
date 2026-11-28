use super::cwd::forget_sessions_async;
use super::fs::run_blocking;
use super::lock;
use crate::error::Result;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::{Duration, SystemTime};

const SESSION_NAMED_PREFIX: &[u8] = br#"{"record_type":"session_named""#;

struct PruneContext<'a> {
    active_session_id: &'a str,
    cutoff: SystemTime,
}

fn is_session_candidate(path: &Path, meta: &std::fs::Metadata, active_session_id: Option<&str>) -> bool {
    meta.is_file()
        && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        && active_session_id.is_none_or(|active| path.file_stem().and_then(|stem| stem.to_str()) != Some(active))
}

fn should_prune_empty_session(
    path: &Path,
    active_session_id: Option<&str>,
    metadata: Option<std::fs::Metadata>,
) -> bool {
    let Some(meta) = metadata.filter(|meta| is_session_candidate(path, meta, active_session_id)) else {
        return false;
    };
    if meta.len() > 300 {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    !["\"tree_node\"", "\"checkpoint_node\"", "\"session_named\""]
        .iter()
        .any(|record_type| content.contains(record_type))
}

/// Removes an unheld session file, noting its id so the working-directory index can drop it.
fn remove_session_file(path: &Path, removed: &mut BTreeSet<String>) {
    if let Some(id) = path.file_stem().and_then(|stem| stem.to_str())
        && lock::remove_unless_held(path)
    {
        removed.insert(id.to_string());
    }
}

async fn forget_removed(sessions_dir: &Path, removed: BTreeSet<String>) -> Result<usize> {
    let count = removed.len();
    forget_sessions_async(sessions_dir, removed).await?;
    Ok(count)
}

pub async fn prune_empty_sessions_async(sessions_dir: &Path, active_session_id: Option<&str>) -> Result<usize> {
    if !tokio::fs::try_exists(sessions_dir).await? {
        return Ok(0);
    }
    let mut entries = tokio::fs::read_dir(sessions_dir).await?;
    let mut removed = BTreeSet::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await.ok();
        if should_prune_empty_session(&path, active_session_id, metadata) {
            remove_session_file(&path, &mut removed);
        }
    }
    forget_removed(sessions_dir, removed).await
}

fn calculate_prune_cutoff(retention_days: u32) -> Option<SystemTime> {
    if retention_days == 0 {
        return None;
    }
    SystemTime::now().checked_sub(Duration::from_secs(retention_days as u64 * 86_400))
}

async fn drain_prune_entries_async(
    ctx: &PruneContext<'_>,
    mut entries: tokio::fs::ReadDir,
) -> Result<BTreeSet<String>> {
    let mut removed = BTreeSet::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await.ok();
        if ctx.should_prune_async(&path, metadata).await {
            remove_session_file(&path, &mut removed);
        }
    }
    Ok(removed)
}

pub async fn prune_expired_sessions_async(
    sessions_dir: &Path,
    active_session_id: &str,
    retention_days: u32,
) -> Result<usize> {
    let Some(cutoff) = calculate_prune_cutoff(retention_days) else {
        return Ok(0);
    };
    if !tokio::fs::try_exists(sessions_dir).await? {
        return Ok(0);
    }

    let ctx = PruneContext {
        active_session_id,
        cutoff,
    };
    let entries = tokio::fs::read_dir(sessions_dir).await?;
    let removed = drain_prune_entries_async(&ctx, entries).await?;
    forget_removed(sessions_dir, removed).await
}

impl PruneContext<'_> {
    async fn should_prune_async(&self, path: &Path, metadata: Option<std::fs::Metadata>) -> bool {
        let Some(meta) = metadata.filter(|meta| is_session_candidate(path, meta, Some(self.active_session_id))) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        if modified >= self.cutoff {
            return false;
        }
        let path = path.to_path_buf();
        !run_blocking(move || Ok(is_named_session_file(&path)))
            .await
            .unwrap_or(true)
    }
}

/// Reports whether any record in the file is a session name, reading only the start of each line.
fn is_named_session_file(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut prefix = Vec::with_capacity(SESSION_NAMED_PREFIX.len());
    loop {
        prefix.clear();
        let limit = SESSION_NAMED_PREFIX.len() as u64;
        match (&mut reader).take(limit).read_until(b'\n', &mut prefix) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        if prefix.starts_with(SESSION_NAMED_PREFIX) {
            return true;
        }
        if prefix.last() != Some(&b'\n') && reader.skip_until(b'\n').is_err() {
            return false;
        }
    }
}
