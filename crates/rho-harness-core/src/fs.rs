//! Crash-safe replacement of rho state files.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Replaces `path` with `contents` via a same-directory temp file, fsync, and rename.
///
/// The temp file is created with `mode` (unix permission bits) and chmod'ed explicitly,
/// so the replacement carries that mode even when the previous file was more permissive.
pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = write_temp_then_rename(&temp, &target, &dir, contents, mode);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Async wrapper that keeps the blocking filesystem work off the runtime threads.
pub async fn write_atomic_async(path: &Path, contents: Vec<u8>, mode: u32) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_atomic(&path, &contents, mode))
        .await
        .map_err(io::Error::other)?
}

fn write_temp_then_rename(temp: &Path, target: &Path, dir: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let mut file = create_temp(temp, mode)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temp, target)?;
    sync_dir(dir)
}

fn create_temp(temp: &Path, mode: u32) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(mode);
        let file = options.open(temp)?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        options.open(temp)
    }
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("rho_fs_test_{}", uuid::Uuid::new_v4()))
    }

    #[cfg(unix)]
    #[test]
    fn replaces_permissive_file_with_requested_mode_and_no_leftover_temp() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_atomic(&path, b"secret", 0o600).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["state.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = temp_dir();
        let path = dir.join("nested").join("index.json");

        write_atomic(&path, b"{}", 0o600).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
