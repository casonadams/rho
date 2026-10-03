use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use crate::tools::artifact::{cleanup_artifacts, spill_artifact};

const DEFAULT_RETENTION_SECS: u64 = 7 * 24 * 3600;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

pub struct ArtifactDemotionHook {
    dir: PathBuf,
}

impl ArtifactDemotionHook {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = cleanup_artifacts(
            &dir,
            Duration::from_secs(DEFAULT_RETENTION_SECS),
            DEFAULT_MAX_TOTAL_BYTES,
        );
        Self { dir }
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
        _conversation_id: &'a rig::id::ConversationId,
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
    use crate::tools::artifact::collect_artifact_files;

    #[tokio::test]
    async fn test_demotion_hook_persists_messages() {
        use rig::memory::DemotionHook;
        let dir = tempfile::tempdir().unwrap();
        let hook = ArtifactDemotionHook::new(dir.path());

        let messages = vec![rig::message::Message::user("Hello from evicted context")];
        let cid = rig::id::ConversationId::from("conv-1");
        let _ = hook.on_demote(&cid, messages).await;

        let files = collect_artifact_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        let path = &files[0].path;
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Hello from evicted context"));
    }
}
