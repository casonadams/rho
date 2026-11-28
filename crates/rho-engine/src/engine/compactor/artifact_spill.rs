use rho_harness_core::model::ChatMessage;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use crate::tools::artifact::cleanup_artifacts;
use rho_harness_core::fs::write_atomic_async;

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
    pub async fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let sweep_dir = dir.clone();
        let swept = match tokio::task::spawn_blocking(move || {
            cleanup_artifacts(
                &sweep_dir,
                Duration::from_secs(DEFAULT_RETENTION_SECS),
                DEFAULT_MAX_TOTAL_BYTES,
                Duration::ZERO,
            )
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(std::io::Error::other(error)),
        };
        if let Err(error) = swept {
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
            let serialized =
                serde_json::to_vec_pretty(&messages).map_err(|e| format!("serializing demoted messages: {e}"))?;
            let path = self.dir.join(format!("{}.json", uuid::Uuid::new_v4()));
            write_atomic_async(&path, serialized, 0o600)
                .await
                .map_err(|e| format!("spilling demoted messages: {e}"))
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
        let hook = ArtifactDemotionHook::new(blocker.join("artifacts")).await;
        let messages = [ChatMessage::user("hi")];
        assert!(hook.on_demote("session", &messages).await.is_err());
    }

    async fn spilled_file(dir: &std::path::Path) -> std::path::PathBuf {
        let hook = ArtifactDemotionHook::new(dir.join("artifacts")).await;
        hook.on_demote("session", &[ChatMessage::user("hi")]).await.unwrap();
        let written: Vec<_> = std::fs::read_dir(dir.join("artifacts"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(written.len(), 1);
        written.into_iter().next().unwrap()
    }

    #[tokio::test]
    async fn a_spill_is_written_as_a_json_file() {
        let dir = rho_test_support::TestDir::new();
        let spilled = spilled_file(&dir).await;
        assert_eq!(spilled.extension().and_then(|e| e.to_str()), Some("json"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_spill_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = rho_test_support::TestDir::new();
        let spilled = spilled_file(&dir).await;
        let mode = std::fs::metadata(&spilled).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
