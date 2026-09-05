use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default, alias = "model", skip_serializing_if = "Option::is_none")]
    pub last_model: Option<String>,
    #[serde(default, alias = "provider", skip_serializing_if = "Option::is_none")]
    pub last_provider: Option<String>,
    #[serde(
        default,
        alias = "thinking_level",
        alias = "thinking",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_thinking_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<serde_json::Value>,
}

impl AppState {
    pub fn load(config_dir: &Path) -> Self {
        let state_file = config_dir.join("state.json");
        if !state_file.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub async fn load_async(config_dir: &Path) -> Self {
        let state_file = config_dir.join("state.json");
        if !tokio::fs::try_exists(&state_file).await.unwrap_or(false) {
            return Self::default();
        }
        tokio::fs::read_to_string(&state_file)
            .await
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let state_file = config_dir.join("state.json");
        let content = serde_json::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        std::fs::write(&state_file, content)?;
        Ok(())
    }

    pub async fn save_async(&self, config_dir: &Path) -> Result<()> {
        tokio::fs::create_dir_all(config_dir).await?;
        let state_file = config_dir.join("state.json");
        let content = serde_json::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        tokio::fs::write(&state_file, content).await?;
        Ok(())
    }

    pub fn set_last_model(config_dir: &Path, model: &str, provider: Option<&str>) -> Result<()> {
        let mut state = Self::load(config_dir);
        state.last_model = Some(model.to_string());
        if let Some(p) = provider {
            state.last_provider = Some(p.to_string());
        } else if let Some(inferred) = crate::provider::infer_provider_for_model(model) {
            state.last_provider = Some(inferred.to_string());
        }
        state.save(config_dir)
    }

    pub async fn set_last_model_async(config_dir: &Path, model: &str, provider: Option<&str>) -> Result<()> {
        let mut state = Self::load_async(config_dir).await;
        state.last_model = Some(model.to_string());
        if let Some(p) = provider {
            state.last_provider = Some(p.to_string());
        } else if let Some(inferred) = crate::provider::infer_provider_for_model(model) {
            state.last_provider = Some(inferred.to_string());
        }
        state.save_async(config_dir).await
    }

    pub fn set_last_thinking_level(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let mut state = Self::load(config_dir);
        state.last_thinking_level = thinking_level.map(ToString::to_string);
        state.save(config_dir)
    }

    pub async fn set_last_thinking_level_async(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let mut state = Self::load_async(config_dir).await;
        state.last_thinking_level = thinking_level.map(ToString::to_string);
        state.save_async(config_dir).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("state_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn state_roundtrips_and_loads_default_when_missing() {
        let dir = temp_dir();
        assert_eq!(AppState::load(&dir), AppState::default());

        AppState::set_last_model(&dir, "gemini-2.0-flash", Some("gemini")).unwrap();
        AppState::set_last_thinking_level(&dir, Some("high")).unwrap();

        let loaded = AppState::load(&dir);
        assert_eq!(loaded.last_model.as_deref(), Some("gemini-2.0-flash"));
        assert_eq!(loaded.last_provider.as_deref(), Some("gemini"));
        assert_eq!(loaded.last_thinking_level.as_deref(), Some("high"));

        let aliased: AppState =
            serde_json::from_str(r#"{"model":"gpt-4o","provider":"openai","thinking":"medium"}"#).unwrap();
        assert_eq!(aliased.last_model.as_deref(), Some("gpt-4o"));
        assert_eq!(aliased.last_provider.as_deref(), Some("openai"));
        assert_eq!(aliased.last_thinking_level.as_deref(), Some("medium"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
