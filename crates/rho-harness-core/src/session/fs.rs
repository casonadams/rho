use crate::error::{AppError, Result};
use chrono::Utc;
use std::path::Path;

pub(crate) fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn set_private_file_permissions(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if _path.exists() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

pub(crate) fn new_session_id() -> String {
    format!(
        "{}_{}",
        Utc::now().format("%Y%m%d_%H%M%S"),
        &uuid::Uuid::new_v4().to_string()[..8]
    )
}

pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| session_error(format!("session task failed: {e}")))?
}

pub(crate) fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(session_error("invalid session id"));
    }
    Ok(())
}

pub(crate) fn session_error(message: impl Into<String>) -> AppError {
    AppError::Session(message.into())
}

#[cfg(test)]
mod tests {
    use super::run_blocking;

    #[tokio::test]
    async fn blocking_task_result_is_returned() {
        assert_eq!(run_blocking(|| Ok(7)).await.unwrap(), 7);
    }

    #[tokio::test]
    async fn blocking_task_panic_surfaces_as_session_error() {
        let error = run_blocking(|| -> crate::error::Result<()> { panic!("worker failed") })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("session task failed"), "{error}");
    }
}
