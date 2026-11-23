//! Atomic file replacement: the content goes to a sibling temporary file, is synced, and is
//! renamed over the target, which keeps the target's permissions on Unix.

use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

fn temp_file_for_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()))
}

/// The mode the existing target has, which the replacement must never be wider than.
#[cfg(unix)]
async fn existing_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::metadata(path)
        .await
        .ok()
        .map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
async fn existing_mode(_path: &Path) -> Option<u32> {
    None
}

async fn create_temp(temp_path: &Path, mode: Option<u32>) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(temp_path).await
}

async fn write_and_sync_temp(temp_path: &Path, content: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let mut file = create_temp(temp_path, mode).await?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(mode)).await?;
    }
    file.write_all(content).await?;
    file.flush().await?;
    if !rho_harness_core::fs::FSYNC_ENABLED {
        return Ok(());
    }
    file.sync_all().await
}

pub async fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let temp_path = temp_file_for_path(path);
    let mode = existing_mode(path).await;
    let written = write_and_sync_temp(&temp_path, content, mode).await;
    let renamed = match written {
        Ok(()) => tokio::fs::rename(&temp_path, path).await,
        Err(error) => Err(error),
    };
    if renamed.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    renamed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_atomic_write_creates_and_overwrites() {
        let temp_dir = crate::test_support::TestDir::new();
        let target = temp_dir.join("test.txt");

        atomic_write(&target, b"initial").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"initial");

        atomic_write(&target, b"updated content").await.unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"updated content");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_existing_files_mode_is_kept_from_the_first_byte_written() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::test_support::TestDir::new();
        for mode in [0o600, 0o640, 0o755, 0o400] {
            let target = dir.join(format!("f{mode:o}"));
            std::fs::write(&target, "old").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).unwrap();

            atomic_write(&target, b"new secret").await.unwrap();

            assert_eq!(std::fs::read(&target).unwrap(), b"new secret");
            let kept = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
            assert_eq!(kept, mode, "mode {mode:o}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_temporary_file_is_never_wider_than_the_target() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::test_support::TestDir::new();
        let target = dir.join("secret.txt");
        std::fs::write(&target, "old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let temp = dir.join("probe.tmp");

        let file = create_temp(&temp, Some(0o600)).await.unwrap();
        drop(file);

        let mode = std::fs::metadata(&temp).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "group and other get no access, got {mode:o}");
    }

    #[tokio::test]
    async fn a_failed_write_leaves_no_temporary_file_behind() {
        let dir = crate::test_support::TestDir::new();
        let missing = dir.join("no-such-dir").join("file.txt");

        assert!(atomic_write(&missing, b"x").await.is_err());

        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}
