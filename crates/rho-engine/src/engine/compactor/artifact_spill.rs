use rho_harness_core::model::ChatMessage;
use std::future::Future;
use std::path::PathBuf;
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
        if let Err(error) = cleanup_artifacts(
            &dir,
            Duration::from_secs(DEFAULT_RETENTION_SECS),
            DEFAULT_MAX_TOTAL_BYTES,
            Duration::ZERO,
        ) {
            tracing::warn!("cleaning up artifacts in {} failed: {error}", dir.display());
        }
        Self { dir }
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
            let serialized =
                serde_json::to_string_pretty(&messages).map_err(|e| format!("serializing demoted messages: {e}"))?;
            spill_artifact(&self.dir, &serialized).map_err(|e| format!("spilling demoted messages: {e}"))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_spill_that_cannot_be_written_is_reported_to_the_caller() {
        let dir = rho_test_support::TestDir::new();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "file").unwrap();
        let hook = ArtifactDemotionHook::new(blocker.join("artifacts"));
        let messages = [ChatMessage::user("hi")];
        assert!(hook.on_demote("session", &messages).await.is_err());
    }

    #[tokio::test]
    async fn a_spill_writes_the_demoted_messages() {
        let dir = rho_test_support::TestDir::new();
        let hook = ArtifactDemotionHook::new(dir.join("artifacts"));
        let messages = [ChatMessage::user("hi")];
        hook.on_demote("session", &messages).await.unwrap();
        let written: Vec<_> = std::fs::read_dir(dir.join("artifacts")).unwrap().collect();
        assert_eq!(written.len(), 1);
    }
}
