pub mod client;
pub mod manager;

pub use client::LspClient;
pub use manager::LspManager;

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

impl LspDiagnostic {
    pub fn severity_str(&self) -> &'static str {
        match self.severity {
            Some(1) => "error",
            Some(2) => "warning",
            Some(3) => "info",
            Some(4) => "hint",
            _ => "diagnostic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspSymbol {
    pub name: String,
    pub kind: u64,
    pub range: LspRange,
}

static KIND_NAMES: &[&str] = &[
    "Symbol",
    "File",
    "Module",
    "Namespace",
    "Package",
    "Class",
    "Method",
    "Property",
    "Field",
    "Constructor",
    "Enum",
    "Interface",
    "Function",
    "Variable",
    "Constant",
    "String",
    "Number",
    "Boolean",
    "Array",
    "Object",
    "Key",
    "Null",
    "EnumMember",
    "Struct",
    "Event",
    "Operator",
    "TypeParameter",
];

pub fn symbol_kind_name(kind: u64) -> &'static str {
    let idx = kind as usize;
    if idx < KIND_NAMES.len() {
        KIND_NAMES[idx]
    } else {
        "Symbol"
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_severity_and_symbol_kinds() {
        let d = LspDiagnostic {
            range: LspRange {
                start: LspPosition { line: 0, character: 0 },
                end: LspPosition { line: 0, character: 5 },
            },
            severity: Some(1),
            message: "err".into(),
            source: None,
        };
        assert_eq!(d.severity_str(), "error");
        assert_eq!(
            LspDiagnostic {
                severity: Some(2),
                ..d.clone()
            }
            .severity_str(),
            "warning"
        );
        assert_eq!(
            LspDiagnostic {
                severity: Some(3),
                ..d.clone()
            }
            .severity_str(),
            "info"
        );
        assert_eq!(LspDiagnostic { severity: None, ..d }.severity_str(), "diagnostic");

        let fn_kind = symbol_kind_name(12);
        let struct_kind = symbol_kind_name(23);
        let default_kind = symbol_kind_name(0);
        assert_eq!(
            format!("{fn_kind}-{struct_kind}-{default_kind}"),
            "Function-Struct-Symbol"
        );
    }
}
