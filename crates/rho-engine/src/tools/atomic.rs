use std::path::Path;
use tokio::io::AsyncWriteExt;

/// Atomically writes content to a file by writing to a sibling temporary file
/// in the same directory, syncing to disk, and renaming over the target path.
/// Preserves existing file permissions on Unix if the target file already exists.
pub async fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let temp_name = format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4());
    let temp_path = parent.join(temp_name);

    #[cfg(unix)]
    let existing_perms = match tokio::fs::metadata(path).await {
        Ok(meta) => Some(meta.permissions()),
        Err(_) => None,
    };

    let result = async {
        let mut file = tokio::fs::File::create(&temp_path).await?;
        file.write_all(content).await?;
        file.flush().await?;
        file.sync_all().await?;

        #[cfg(unix)]
        if let Some(perms) = existing_perms {
            let _ = tokio::fs::set_permissions(&temp_path, perms).await;
        }

        tokio::fs::rename(&temp_path, path).await?;
        Ok::<(), std::io::Error>(())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    result
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
