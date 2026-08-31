use super::cargo::CargoRunner;
use crate::process::{PluginProcessClient, ProcessLimits};
use async_trait::async_trait;
use rho_core::config::{Config, PluginConfig};
use rho_core::error::{AppError, Result};
use rho_sdk::capability::{CapabilityId, ValidatedManifest};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait PluginValidator: Send + Sync {
    async fn validate(&self, executable: &Path) -> Result<ValidatedManifest>;
}

pub struct ProtocolPluginValidator;

#[async_trait]
impl PluginValidator for ProtocolPluginValidator {
    async fn validate(&self, executable: &Path) -> Result<ValidatedManifest> {
        validate_executable(executable).await
    }
}

pub struct PluginManagerPaths {
    pub config_dir: PathBuf,
    pub cargo_bin: PathBuf,
}

pub struct PluginManager<R, V> {
    pub(crate) paths: PluginManagerPaths,
    pub(crate) cargo: R,
    pub(crate) validator: V,
}

impl<R, V> PluginManager<R, V>
where
    R: CargoRunner,
    V: PluginValidator,
{
    pub fn new(paths: PluginManagerPaths, cargo: R, validator: V) -> Self {
        Self {
            paths,
            cargo,
            validator,
        }
    }

    pub async fn install(
        &self,
        package: &str,
        authorized_replacements: BTreeSet<CapabilityId>,
    ) -> Result<InstalledPlugin> {
        validate_package(package)?;
        let executable_names = executable_names(package);
        let existed_before = executable_names
            .iter()
            .any(|name| executable_path(&self.paths.cargo_bin, name).is_file());
        self.cargo.install(package)?;
        let newly_installed = !existed_before;
        let outcome = self
            .complete_install(PendingInstall {
                package,
                executable_names: &executable_names,
                authorized_replacements,
            })
            .await;
        match outcome {
            Ok(installed) => Ok(installed),
            Err(error) if newly_installed => match self.cargo.uninstall(package) {
                Ok(()) => Err(error),
                Err(rollback) => Err(AppError::Plugin(format!(
                    "{error}; Cargo uninstall rollback also failed: {rollback}"
                ))),
            },
            Err(error) => Err(error),
        }
    }

    async fn complete_install(&self, pending: PendingInstall<'_>) -> Result<InstalledPlugin> {
        let executable = pending
            .executable_names
            .iter()
            .map(|name| executable_path(&self.paths.cargo_bin, name))
            .find(|path| path.is_file() && executable_is_runnable(path))
            .ok_or_else(|| {
                AppError::Plugin(format!(
                    "Installed package does not provide a runnable {} or {} executable",
                    pending
                        .executable_names
                        .first()
                        .map_or("rho-plugin-<name>", String::as_str),
                    pending.executable_names.get(1).map_or("rho-<name>", String::as_str)
                ))
            })?;
        let executable = std::fs::canonicalize(executable)?;
        let manifest = self.validator.validate(&executable).await?;
        let declared_replacements: BTreeSet<_> = manifest
            .capabilities
            .iter()
            .filter_map(|declaration| declaration.replaces.clone())
            .collect();
        if declared_replacements != pending.authorized_replacements {
            return Err(AppError::Plugin(format!(
                "Authorized replacements do not exactly match plugin metadata: declared [{}], authorized [{}]",
                display_ids(&declared_replacements),
                display_ids(&pending.authorized_replacements)
            )));
        }
        let name = manifest.plugin_id.to_string();
        let plugin = PluginConfig {
            path: executable.clone(),
            package: Some(pending.package.to_string()),
            replaces: pending.authorized_replacements,
            ..Default::default()
        };
        Config::add_plugin(&self.paths.config_dir, &name, plugin)?;
        Ok(InstalledPlugin {
            name,
            executable,
            manifest,
        })
    }

    pub fn remove(&self, name: &str) -> Result<RemovedPlugin> {
        let plugin = Config::remove_plugin(&self.paths.config_dir, name)?;
        if let Some(package) = &plugin.package
            && let Err(error) = self.cargo.uninstall(package)
        {
            return Err(AppError::Plugin(format!(
                "Plugin declaration was removed, but Cargo uninstall failed: {error}"
            )));
        }
        Ok(RemovedPlugin {
            name: name.to_string(),
            path: plugin.path,
            package: plugin.package,
        })
    }
}

pub(crate) struct PendingInstall<'a> {
    pub(crate) package: &'a str,
    pub(crate) executable_names: &'a [String],
    pub(crate) authorized_replacements: BTreeSet<CapabilityId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub name: String,
    pub executable: PathBuf,
    pub manifest: ValidatedManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedPlugin {
    pub name: String,
    pub path: PathBuf,
    pub package: Option<String>,
}

pub(crate) fn validate_package(package: &str) -> Result<()> {
    if package.is_empty()
        || package.len() > 128
        || !package.is_ascii()
        || !package.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::Plugin("Cargo package name is invalid".to_string()));
    }
    Ok(())
}

pub(crate) fn executable_names(package: &str) -> Vec<String> {
    let package = package.replace('_', "-");
    let suffix = package
        .strip_prefix("rho-plugin-")
        .or_else(|| package.strip_prefix("rho-"))
        .unwrap_or(&package)
        .to_string();
    let preferred = package.starts_with("rho-").then_some(package);
    let mut names = Vec::new();
    for name in [
        preferred,
        Some(format!("rho-plugin-{suffix}")),
        Some(format!("rho-{suffix}")),
    ]
    .into_iter()
    .flatten()
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

pub(crate) fn executable_path(cargo_bin: &Path, name: &str) -> PathBuf {
    cargo_bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

pub(crate) fn display_ids(ids: &BTreeSet<CapabilityId>) -> String {
    ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(unix)]
pub(crate) fn executable_is_runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub(crate) fn executable_is_runnable(path: &Path) -> bool {
    path.is_file()
}

pub async fn validate_executable(executable: &Path) -> Result<ValidatedManifest> {
    let discovery = PluginProcessClient::new(executable, ProcessLimits::default())
        .discover()
        .await
        .map_err(|error| AppError::Plugin(error.to_string()))?;
    discovery
        .validate_strict()
        .map_err(|error| AppError::Plugin(error.to_string()))?;
    Ok(discovery.manifest)
}
