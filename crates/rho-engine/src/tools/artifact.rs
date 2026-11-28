use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub(crate) fn spill_artifact(dir: &Path, content: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let id = uuid::Uuid::new_v4();
    let file_path = dir.join(format!("{id}.log"));
    std::fs::write(&file_path, content)?;
    Ok(file_path)
}

pub(crate) fn format_artifact_notice(path: &Path, total_lines: usize, total_bytes: usize) -> String {
    let size_str = crate::tools::truncate::format_size(total_bytes);
    let path_str = path.display();
    format!(
        "[Output truncated. Full content ({total_lines} lines, {size_str}) saved to {path_str}. Use 'read' or 'bash' with grep on this file if needed.]"
    )
}

#[derive(Debug)]
pub(crate) struct ArtifactFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

/// A symlinked or foreign-owned `dir` and symlinked entries are skipped: the sweep deletes what it
/// collects, and `dir` has a predictable name under the shared temp directory.
pub(crate) fn collect_artifact_files(dir: &Path) -> std::io::Result<Vec<ArtifactFile>> {
    let mut files = Vec::new();
    let Ok(dir_meta) = std::fs::symlink_metadata(dir) else {
        return Ok(files);
    };
    if !dir_meta.is_dir() || !owned_by_current_user(&dir_meta) {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let metadata = entry.metadata()?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            files.push(ArtifactFile {
                path: entry.path(),
                size: metadata.len(),
                modified,
            });
        }
    }
    Ok(files)
}

#[cfg(unix)]
fn owned_by_current_user(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    meta.uid() == unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn owned_by_current_user(_meta: &std::fs::Metadata) -> bool {
    true
}

fn prune_expired_files(files: &mut Vec<ArtifactFile>, cutoff: SystemTime) -> usize {
    let mut removed = 0;
    files.retain(|file| {
        if file.modified < cutoff && std::fs::remove_file(&file.path).is_ok() {
            removed += 1;
            return false;
        }
        true
    });
    removed
}

fn prune_overflow_files(files: &mut [ArtifactFile], max_bytes: u64, spare_after: SystemTime) -> usize {
    files.sort_by_key(|f| f.modified);
    let mut total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let mut removed = 0;

    let mut idx = 0;
    while total_bytes > max_bytes && idx < files.len() && files[idx].modified < spare_after {
        if std::fs::remove_file(&files[idx].path).is_ok() {
            total_bytes = total_bytes.saturating_sub(files[idx].size);
            removed += 1;
        }
        idx += 1;
    }
    removed
}

/// Removes files older than `max_age`, then the oldest while the total exceeds `max_total_bytes`; files newer
/// than `spare_newer_than` are kept, since another live process may still point the model at them.
pub(crate) fn cleanup_artifacts(
    dir: &Path,
    max_age: Duration,
    max_total_bytes: u64,
    spare_newer_than: Duration,
) -> std::io::Result<usize> {
    let mut files = collect_artifact_files(dir)?;
    let now = SystemTime::now();
    let cutoff = now.checked_sub(max_age).unwrap_or(SystemTime::UNIX_EPOCH);
    let spare_after = now.checked_sub(spare_newer_than).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut total_removed = prune_expired_files(&mut files, cutoff);
    total_removed += prune_overflow_files(&mut files, max_total_bytes, spare_after);
    Ok(total_removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spill_artifact_creates_readable_file() {
        let dir = crate::test_support::TestDir::new();
        let content = "Hello, world of lossless artifact spilling!\nLine 2";
        let path = spill_artifact(&dir, content).unwrap();

        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn test_format_artifact_notice_structure() {
        let path = Path::new("/tmp/test-artifact.log");
        let notice = format_artifact_notice(path, 120, 4500);

        assert!(notice.contains("120 lines"));
        assert!(notice.contains("4.4KB"));
        assert!(notice.contains("/tmp/test-artifact.log"));
    }

    #[test]
    fn test_cleanup_artifacts_by_age_and_size() {
        let dir = crate::test_support::TestDir::new();
        let path1 = dir.join("a1.log");
        let path2 = dir.join("a2.log");
        let path3 = dir.join("a3.log");

        std::fs::write(&path1, vec![b'a'; 1000]).unwrap();
        std::fs::write(&path2, vec![b'b'; 1000]).unwrap();
        std::fs::write(&path3, vec![b'c'; 1000]).unwrap();

        let removed = cleanup_artifacts(&dir, Duration::from_secs(3600), 1500, Duration::ZERO).unwrap();
        assert!(removed >= 1);

        let files = collect_artifact_files(&dir).unwrap();
        let total_remaining: u64 = files.iter().map(|f| f.size).sum();
        assert!(total_remaining <= 2000);
    }

    #[test]
    fn recent_files_are_spared_even_when_over_the_size_cap() {
        let dir = crate::test_support::TestDir::new();
        for name in ["a.log", "b.log", "c.log"] {
            std::fs::write(dir.join(name), vec![b'x'; 1000]).unwrap();
        }

        let removed = cleanup_artifacts(&dir, Duration::from_secs(3600), 1500, Duration::from_secs(3600)).unwrap();

        assert_eq!(removed, 0);
        assert_eq!(collect_artifact_files(&dir).unwrap().len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_a_symlinked_directory_or_symlinked_files() {
        let root = crate::test_support::TestDir::new();
        let victim_dir = root.join("victim");
        std::fs::create_dir(&victim_dir).unwrap();
        let victim_file = victim_dir.join("notes.txt");
        std::fs::write(&victim_file, "precious").unwrap();

        let log_dir = root.join("rho-bash");
        std::os::unix::fs::symlink(&victim_dir, &log_dir).unwrap();
        let removed = cleanup_artifacts(&log_dir, Duration::ZERO, 0, Duration::ZERO).unwrap();
        assert_eq!(removed, 0);
        assert!(victim_file.exists());

        let real_dir = root.join("real");
        std::fs::create_dir(&real_dir).unwrap();
        std::os::unix::fs::symlink(&victim_file, real_dir.join("link.log")).unwrap();
        let removed = cleanup_artifacts(&real_dir, Duration::ZERO, 0, Duration::ZERO).unwrap();
        assert_eq!(removed, 0);
        assert!(victim_file.exists());
    }

    #[test]
    fn test_collect_artifact_files_returns_files() {
        let dir = crate::test_support::TestDir::new();
        let file = dir.join("f1.log");
        std::fs::write(&file, "test data").unwrap();

        let files = collect_artifact_files(&dir).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, file);
        assert_eq!(files[0].size, 9);
    }
}
