use std::path::Path;
use tokio::io::AsyncWriteExt;

/// Atomically writes content to a file by writing to a sibling temporary file
/// in the same directory, syncing to disk, and renaming over the target path.
/// Preserves existing file permissions on Unix if the target file already exists.
fn temp_file_for_path(path: &Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()))
}

async fn write_and_sync_temp(temp_path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = tokio::fs::File::create(temp_path).await?;
    file.write_all(content).await?;
    file.flush().await?;
    file.sync_all().await
}

#[cfg(unix)]
async fn read_existing_perms(path: &Path) -> Option<std::fs::Permissions> {
    tokio::fs::metadata(path).await.ok().map(|m| m.permissions())
}

async fn finalize_atomic_replace(
    temp_path: &Path,
    path: &Path,
    perms: Option<std::fs::Permissions>,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(p) = perms {
        let _ = tokio::fs::set_permissions(temp_path, p).await;
    }
    if let Err(e) = tokio::fs::rename(temp_path, path).await {
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(e);
    }
    Ok(())
}

pub async fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let temp_path = temp_file_for_path(path);
    #[cfg(unix)]
    let perms = read_existing_perms(path).await;
    #[cfg(not(unix))]
    let perms: Option<std::fs::Permissions> = None;

    if let Err(e) = write_and_sync_temp(&temp_path, content).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(e);
    }
    finalize_atomic_replace(&temp_path, path, perms).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_atomic_write_creates_and_overwrites() {
        let temp_dir = std::env::temp_dir().join(format!("atomic_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let target = temp_dir.join("test.txt");

        atomic_write(&target, b"initial").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"initial");

        atomic_write(&target, b"updated content").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"updated content");

        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
