use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ExtensionContext {
    pub cwd: PathBuf,
    pub session_id: String,
    pub model: String,
    pub provider: String,
    pub has_ui: bool,
    pub is_trusted: bool,
}

impl ExtensionContext {
    pub fn new(cwd: impl AsRef<Path>, session_id: impl Into<String>) -> Self {
        Self {
            cwd: cwd.as_ref().to_path_buf(),
            session_id: session_id.into(),
            model: String::new(),
            provider: String::new(),
            has_ui: true,
            is_trusted: true,
        }
    }

    pub fn with_model_info(mut self, model: impl Into<String>, provider: impl Into<String>) -> Self {
        self.model = model.into();
        self.provider = provider.into();
        self
    }

    pub fn with_ui(mut self, has_ui: bool) -> Self {
        self.has_ui = has_ui;
        self
    }

    pub fn with_trusted(mut self, is_trusted: bool) -> Self {
        self.is_trusted = is_trusted;
        self
    }
}
