use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let state_file = config_dir.join("state.json");
        let content = serde_json::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        std::fs::write(&state_file, content)?;
        Ok(())
    }

    pub fn set_last_model(config_dir: &Path, model: &str, provider: Option<&str>) -> Result<()> {
        let mut state = Self::load(config_dir);
        state.last_model = Some(model.to_string());
        if let Some(p) = provider {
            state.last_provider = Some(p.to_string());
        }
        state.save(config_dir)
    }

    pub fn set_last_thinking_level(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let mut state = Self::load(config_dir);
        state.last_thinking_level = thinking_level.map(ToString::to_string);
        state.save(config_dir)
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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
