use super::paths::{is_in_cargo_bin, resolve_cargo_bin_dir, resolve_plugin_binary_path};
use rho_harness_core::config::{Config, PluginConfig};
use rho_harness_core::error::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalArtifactStatus {
    Deleted(PathBuf),
    Kept(PathBuf),
    PreservedExternal(PathBuf),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRemovalResult {
    pub name: String,
    pub artifact_status: RemovalArtifactStatus,
}

pub fn resolve_plugin_key(name: &str, plugins: &BTreeMap<String, PluginConfig>) -> Option<String> {
    if plugins.contains_key(name) {
        return Some(name.to_string());
    }
    let prefixed = format!("rho-plugin-{name}");
    if plugins.contains_key(&prefixed) {
        return Some(prefixed);
    }
    if let Some(stripped) = name.strip_prefix("rho-plugin-")
        && plugins.contains_key(stripped)
    {
        return Some(stripped.to_string());
    }
    None
}

#[derive(Debug, Clone, Copy)]
pub struct RemovePluginContext<'a> {
    pub config_dir: &'a Path,
    pub plugins: &'a BTreeMap<String, PluginConfig>,
    pub keep_binary: bool,
    pub cargo_bin_dir: Option<&'a Path>,
    pub home_dir: Option<&'a Path>,
}

async fn handle_binary_removal(path: PathBuf, is_managed: bool, keep_binary: bool) -> Result<RemovalArtifactStatus> {
    if !is_managed {
        return Ok(RemovalArtifactStatus::PreservedExternal(path));
    }
    if keep_binary {
        return Ok(RemovalArtifactStatus::Kept(path));
    }
    tokio::fs::remove_file(&path).await?;
    Ok(RemovalArtifactStatus::Deleted(path))
}

async fn determine_artifact_status(
    binary_path: Option<PathBuf>,
    cargo_bin_dir: Option<&Path>,
    keep_binary: bool,
) -> Result<RemovalArtifactStatus> {
    let Some(path) = binary_path.filter(|p| p.is_file()) else {
        return Ok(RemovalArtifactStatus::NotFound);
    };
    let is_managed = cargo_bin_dir.is_some_and(|bin_dir| is_in_cargo_bin(&path, bin_dir));
    handle_binary_removal(path, is_managed, keep_binary).await
}

pub async fn remove_plugin(ctx: RemovePluginContext<'_>, name: &str) -> Result<PluginRemovalResult> {
    let key = resolve_plugin_key(name, ctx.plugins).unwrap_or_else(|| name.to_string());
    let removed_config = Config::remove_plugin_async(ctx.config_dir, &key).await?;

    let binary_path = resolve_plugin_binary_path(&removed_config, ctx.cargo_bin_dir, ctx.home_dir);
    let artifact_status = determine_artifact_status(binary_path, ctx.cargo_bin_dir, ctx.keep_binary).await?;

    Ok(PluginRemovalResult {
        name: key,
        artifact_status,
    })
}

fn print_removal_status(name: &str, status: &RemovalArtifactStatus) {
    match status {
        RemovalArtifactStatus::Deleted(path) => {
            println!("Removed plugin '{name}' and deleted binary at {}", path.display());
        }
        RemovalArtifactStatus::Kept(path) => {
            println!(
                "Removed plugin '{name}' from configuration (kept binary at {})",
                path.display()
            );
        }
        RemovalArtifactStatus::PreservedExternal(path) => {
            println!(
                "Removed plugin '{name}' from configuration (preserved external binary at {})",
                path.display()
            );
        }
        RemovalArtifactStatus::NotFound => {
            println!("Removed plugin '{name}' from configuration");
        }
    }
}

pub async fn handle_remove(config: &Config, name: &str, keep_binary: bool) -> Result<PluginRemovalResult> {
    let cargo_bin = resolve_cargo_bin_dir();
    let home = dirs::home_dir();
    let result = remove_plugin(
        RemovePluginContext {
            config_dir: &config.config_dir,
            plugins: &config.plugins,
            keep_binary,
            cargo_bin_dir: cargo_bin.as_deref(),
            home_dir: home.as_deref(),
        },
        name,
    )
    .await?;

    print_removal_status(&result.name, &result.artifact_status);
    Ok(result)
}

#[cfg(test)]
#[path = "remove/tests.rs"]
mod tests;
