use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: LspRange,
    pub severity: Option<u8>,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

pub struct LspProcess;

impl LspProcess {
    pub fn auto_detect_command(root: &Path) -> Option<(&'static str, &'static [&'static str])> {
        if root.join("Cargo.toml").exists() {
            Some(("rust-analyzer", &[]))
        } else if root.join("package.json").exists() || root.join("tsconfig.json").exists() {
            Some(("typescript-language-server", &["--stdio"]))
        } else if root.join("go.mod").exists() {
            Some(("gopls", &[]))
        } else if root.join("pyproject.toml").exists() || root.join("requirements.txt").exists() {
            Some(("pyright-langserver", &["--stdio"]))
        } else {
            None
        }
    }
}
