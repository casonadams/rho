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

pub async fn remove_plugin(ctx: RemovePluginContext<'_>, name: &str) -> Result<PluginRemovalResult> {
    let key = resolve_plugin_key(name, ctx.plugins).unwrap_or_else(|| name.to_string());
    let removed_config = Config::remove_plugin_async(ctx.config_dir, &key).await?;

    let binary_path = resolve_plugin_binary_path(&removed_config, ctx.cargo_bin_dir, ctx.home_dir);
    let artifact_status = match binary_path {
        Some(ref path) if path.is_file() => {
            let is_managed = ctx
                .cargo_bin_dir
                .map(|bin_dir| is_in_cargo_bin(path, bin_dir))
                .unwrap_or(false);

            if is_managed {
                if ctx.keep_binary {
                    RemovalArtifactStatus::Kept(path.clone())
                } else {
                    tokio::fs::remove_file(path).await?;
                    RemovalArtifactStatus::Deleted(path.clone())
                }
            } else {
                RemovalArtifactStatus::PreservedExternal(path.clone())
            }
        }
        _ => RemovalArtifactStatus::NotFound,
    };

    Ok(PluginRemovalResult {
        name: key,
        artifact_status,
    })
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

    match &result.artifact_status {
        RemovalArtifactStatus::Deleted(path) => {
            println!(
                "Removed plugin '{}' and deleted binary at {}",
                result.name,
                path.display()
            );
        }
        RemovalArtifactStatus::Kept(path) => {
            println!(
                "Removed plugin '{}' from configuration (kept binary at {})",
                result.name,
                path.display()
            );
        }
        RemovalArtifactStatus::PreservedExternal(path) => {
            println!(
                "Removed plugin '{}' from configuration (preserved external binary at {})",
                result.name,
                path.display()
            );
        }
        RemovalArtifactStatus::NotFound => {
            println!("Removed plugin '{}' from configuration", result.name);
        }
    }

    Ok(result)
}

#[cfg(test)]
#[path = "remove/tests.rs"]
mod tests;
