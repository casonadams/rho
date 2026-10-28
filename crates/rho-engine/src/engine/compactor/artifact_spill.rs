use rho_harness_core::model::ChatMessage;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use crate::tools::artifact::{cleanup_artifacts, spill_artifact};

const DEFAULT_RETENTION_SECS: u64 = 7 * 24 * 3600;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024; // 100 MB

pub trait DemotionHook: Send + Sync {
    fn on_demote<'a>(
        &'a self,
        session_id: &'a str,
        messages: &'a [ChatMessage],
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

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

impl DemotionHook for ArtifactDemotionHook {
    fn on_demote<'a>(
        &'a self,
        _session_id: &'a str,
        messages: &'a [ChatMessage],
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
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
