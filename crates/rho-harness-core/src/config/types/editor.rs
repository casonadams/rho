use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EditorConfig {
    #[serde(default)]
    pub mode: Option<String>,
}

impl EditorConfig {
    pub fn merge(&mut self, other: &EditorConfig) {
        if let Some(ref mode) = other.mode {
            self.mode = Some(mode.clone());
        }
    }

    pub fn is_vim(&self) -> bool {
        self.mode
            .as_deref()
            .map(|m| m.eq_ignore_ascii_case("vim"))
            .unwrap_or(false)
    }
}
