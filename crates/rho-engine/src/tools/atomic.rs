//! Atomic file replacement that keeps an existing target's permissions: the replacement is written
//! through the harness-core atomic writer with the target's mode, so it is never wider than the target.

use std::path::Path;

const NEW_FILE_MODE: u32 = 0o644;

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

pub async fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mode = existing_mode(path).await.unwrap_or(NEW_FILE_MODE);
    rho_harness_core::fs::write_atomic_async(path, content.to_vec(), mode).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_atomic_write_creates_and_overwrites() {
        let temp_dir = rho_test_support::TestDir::new();
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
        let dir = rho_test_support::TestDir::new();
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

    #[tokio::test]
    async fn a_failed_write_leaves_no_temporary_file_behind() {
        let dir = rho_test_support::TestDir::new();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "file").unwrap();

        assert!(atomic_write(&blocker.join("file.txt"), b"x").await.is_err());

        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }
}
