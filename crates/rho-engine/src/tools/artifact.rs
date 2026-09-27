use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime};

pub fn default_artifact_dir() -> PathBuf {
    PathBuf::from(".rho/artifacts")
}

pub fn spill_artifact(dir: &Path, content: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let id = uuid::Uuid::new_v4();
    let file_path = dir.join(format!("{id}.log"));
    std::fs::write(&file_path, content)?;
    Ok(file_path)
}

pub fn format_artifact_notice(path: &Path, total_lines: usize, total_bytes: usize) -> String {
    let size_str = crate::tools::truncate::format_size(total_bytes);
    let path_str = path.display();
    format!(
        "[Output truncated. Full content ({total_lines} lines, {size_str}) saved to {path_str}. Use 'read' or 'bash' with grep on this file if needed.]"
    )
}

#[derive(Debug)]
struct ArtifactFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn collect_artifact_files(dir: &Path) -> std::io::Result<Vec<ArtifactFile>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let metadata = entry.metadata()?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            files.push(ArtifactFile {
                path,
                size: metadata.len(),
                modified,
            });
        }
    }
    Ok(files)
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

fn prune_overflow_files(files: &mut [ArtifactFile], max_bytes: u64) -> usize {
    files.sort_by_key(|f| f.modified);
    let mut total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let mut removed = 0;

    let mut idx = 0;
    while total_bytes > max_bytes && idx < files.len() {
        if std::fs::remove_file(&files[idx].path).is_ok() {
            total_bytes = total_bytes.saturating_sub(files[idx].size);
            removed += 1;
        }
        idx += 1;
    }
    removed
}

pub fn cleanup_artifacts(dir: &Path, max_age: Duration, max_total_bytes: u64) -> std::io::Result<usize> {
    let mut files = collect_artifact_files(dir)?;
    let cutoff = SystemTime::now().checked_sub(max_age).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut total_removed = prune_expired_files(&mut files, cutoff);
    total_removed += prune_overflow_files(&mut files, max_total_bytes);
    Ok(total_removed)
}

pub struct ArtifactDemotionHook {
    dir: PathBuf,
}

impl ArtifactDemotionHook {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn spill_text(&self, content: &str) -> std::io::Result<PathBuf> {
        spill_artifact(&self.dir, content)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl rig::memory::DemotionHook for ArtifactDemotionHook {
    fn on_demote<'a>(
        &'a self,
        _conversation_id: &'a str,
        messages: Vec<rig::message::Message>,
    ) -> Pin<Box<dyn Future<Output = Result<(), rig::memory::MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            if messages.is_empty() {
                return Ok(());
            }
            if let Ok(serialized) = serde_json::to_string_pretty(&messages) {
                let _ = spill_artifact(&self.dir, &serialized);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spill_artifact_creates_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Hello, world of lossless artifact spilling!\nLine 2";
        let path = spill_artifact(dir.path(), content).unwrap();

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
        let dir = tempfile::tempdir().unwrap();
        let path1 = dir.path().join("a1.log");
        let path2 = dir.path().join("a2.log");
        let path3 = dir.path().join("a3.log");

        std::fs::write(&path1, vec![b'a'; 1000]).unwrap();
        std::fs::write(&path2, vec![b'b'; 1000]).unwrap();
        std::fs::write(&path3, vec![b'c'; 1000]).unwrap();

        let removed = cleanup_artifacts(dir.path(), Duration::from_secs(3600), 1500).unwrap();
        assert!(removed >= 1);

        let files = collect_artifact_files(dir.path()).unwrap();
        let total_remaining: u64 = files.iter().map(|f| f.size).sum();
        assert!(total_remaining <= 2000);
    }

    #[tokio::test]
    async fn test_demotion_hook_persists_messages() {
        use rig::memory::DemotionHook;
        let dir = tempfile::tempdir().unwrap();
        let hook = ArtifactDemotionHook::new(dir.path());

        let messages = vec![rig::message::Message::user("Hello from evicted context")];
        let _ = hook.on_demote("conv-1", messages).await;

        let files = collect_artifact_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        let content = std::fs::read_to_string(&files[0].path).unwrap();
        assert!(content.contains("Hello from evicted context"));
    }
}
