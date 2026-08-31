use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    GlobalDirectory,
    WorkspaceDirectory,
    CargoBin,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveredKind {
    Executable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCandidate {
    pub path: PathBuf,
    pub source: DiscoverySource,
    pub kind: DiscoveredKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginDiscovery {
    pub candidates: Vec<DiscoveredCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfiguredStatus {
    Eligible,
    Missing,
    NotAFile,
    NotExecutable,
}

impl ConfiguredStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Missing => "missing",
            Self::NotAFile => "not a file",
            Self::NotExecutable => "not executable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredCandidate {
    pub name: String,
    pub path: PathBuf,
    pub package: Option<String>,
    pub replaces: BTreeSet<rho_sdk::capability::CapabilityId>,
    pub status: ConfiguredStatus,
}

pub(crate) struct DiscoveryPaths<'a> {
    pub(crate) cargo_bin: Option<&'a Path>,
    pub(crate) path_dirs: &'a [PathBuf],
}
