use crate::error::Result;
use std::path::Path;
use std::time::{Duration, SystemTime};

struct PruneContext<'a> {
    active_session_id: &'a str,
    cutoff: SystemTime,
}

pub fn prune_expired_sessions(sessions_dir: &Path, active_session_id: &str, retention_days: u32) -> Result<usize> {
    if retention_days == 0 || !sessions_dir.exists() {
        return Ok(0);
    }
    let Some(cutoff) = SystemTime::now().checked_sub(Duration::from_secs(retention_days as u64 * 86_400)) else {
        return Ok(0);
    };

    let ctx = PruneContext {
        active_session_id,
        cutoff,
    };
    let mut count = 0;
    for entry in std::fs::read_dir(sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if ctx.should_prune(&path, entry.metadata().ok()) && std::fs::remove_file(&path).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

fn calculate_prune_cutoff(retention_days: u32) -> Option<SystemTime> {
    if retention_days == 0 {
        return None;
    }
    SystemTime::now().checked_sub(Duration::from_secs(retention_days as u64 * 86_400))
}

async fn sessions_dir_exists(sessions_dir: &Path) -> bool {
    tokio::fs::try_exists(sessions_dir).await.unwrap_or(false)
}

async fn prune_single_entry_async(ctx: &PruneContext<'_>, entry: tokio::fs::DirEntry) -> bool {
    let path = entry.path();
    let metadata = entry.metadata().await.ok();
    if ctx.should_prune_async(&path, metadata).await {
        tokio::fs::remove_file(&path).await.is_ok()
    } else {
        false
    }
}

async fn drain_prune_entries_async(ctx: &PruneContext<'_>, mut entries: tokio::fs::ReadDir) -> Result<usize> {
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if prune_single_entry_async(ctx, entry).await {
            count += 1;
        }
    }
    Ok(count)
}

pub async fn prune_expired_sessions_async(
    sessions_dir: &Path,
    active_session_id: &str,
    retention_days: u32,
) -> Result<usize> {
    let Some(cutoff) = calculate_prune_cutoff(retention_days) else {
        return Ok(0);
    };
    if !sessions_dir_exists(sessions_dir).await {
        return Ok(0);
    }

    let ctx = PruneContext {
        active_session_id,
        cutoff,
    };
    let entries = tokio::fs::read_dir(sessions_dir).await?;
    drain_prune_entries_async(&ctx, entries).await
}

impl PruneContext<'_> {
    fn should_prune(&self, path: &Path, metadata: Option<std::fs::Metadata>) -> bool {
        let Some(meta) = metadata else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return false;
        }
        if path.file_stem().and_then(|stem| stem.to_str()) == Some(self.active_session_id) {
            return false;
        }
        let Ok(modified) = meta.modified() else {
            return false;
        };
        if modified >= self.cutoff {
            return false;
        }
        !is_named_session_file(path)
    }

    async fn should_prune_async(&self, path: &Path, metadata: Option<std::fs::Metadata>) -> bool {
        let Some(meta) = metadata else {
            return false;
        };
        if !meta.is_file() {
            return false;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            return false;
        }
        if path.file_stem().and_then(|stem| stem.to_str()) == Some(self.active_session_id) {
            return false;
        }
        let Ok(modified) = meta.modified() else {
            return false;
        };
        if modified >= self.cutoff {
            return false;
        }
        !is_named_session_file_async(path).await
    }
}

fn is_named_session_file(path: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.contains("\"session_named\"") {
            return true;
        }
    }
    false
}

async fn is_named_session_file_async(path: &Path) -> bool {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let Ok(file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    while let Ok(bytes_read) = reader.read_line(&mut line).await {
        if bytes_read == 0 {
            break;
        }
        if line.contains("\"session_named\"") {
            return true;
        }
        line.clear();
    }
    false
}
