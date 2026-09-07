use super::archive::extract_binary;
use super::archive::write_binary_atomically;
use super::install::GitHubClient;
use super::install::InstallError;
use super::paths::resolve_cargo_bin_dir;
use super::platform::{Platform, match_platform_asset};
use super::remove::resolve_plugin_key;
use super::spec::PluginSpec;
use super::spec::is_update_available;
use rho_harness_core::config::{Config, PluginConfig};
use rho_harness_core::error::AppError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUpdateStatus {
    AlreadyUpToDate {
        version: String,
    },
    Updated {
        old_version: String,
        new_version: String,
        binary_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUpdateItem {
    pub name: String,
    pub status: PluginUpdateStatus,
}

pub fn resolve_plugin_spec_for_update(name: &str, cfg: &PluginConfig) -> Result<PluginSpec, InstallError> {
    if let Some(git) = &cfg.git
        && let Ok(spec) = PluginSpec::parse(git)
    {
        return Ok(spec);
    }
    if let Some(pkg) = &cfg.package
        && let Ok(spec) = PluginSpec::parse(pkg)
    {
        return Ok(spec);
    }
    if let Some(cmd) = &cfg.command
        && let Ok(spec) = PluginSpec::parse(cmd)
    {
        return Ok(spec);
    }
    PluginSpec::parse(name).map_err(InstallError::from)
}

#[derive(Debug, Clone, Copy)]
pub struct UpdatePluginContext<'a> {
    pub config_dir: &'a Path,
    pub cargo_bin_dir: &'a Path,
    pub client: &'a GitHubClient,
}

async fn download_and_extract_update(
    client: &GitHubClient,
    release: &super::install::Release,
    spec: &PluginSpec,
) -> Result<Vec<u8>, InstallError> {
    let platform = Platform::current().ok_or_else(|| {
        InstallError::UnsupportedPlatform(format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS))
    })?;
    let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
    let matched_name = match_platform_asset(&platform, &asset_names)?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == matched_name)
        .expect("matched asset");
    let downloaded = client.download_asset(&asset.browser_download_url).await?;
    extract_binary(&asset.name, &downloaded, &spec.executable_name).map_err(Into::into)
}

async fn save_updated_config(
    ctx: UpdatePluginContext<'_>,
    (name, cfg): (&str, &PluginConfig),
    version: &str,
) -> Result<(), InstallError> {
    let mut updated = cfg.clone();
    updated.version = Some(version.to_string());
    Config::add_plugin_async(ctx.config_dir, name, updated).await?;
    Ok(())
}

pub async fn update_single_plugin(
    ctx: UpdatePluginContext<'_>,
    name: &str,
    plugin_cfg: &PluginConfig,
) -> Result<PluginUpdateStatus, InstallError> {
    let spec = resolve_plugin_spec_for_update(name, plugin_cfg)?;
    let dest_path = ctx.cargo_bin_dir.join(&spec.executable_name);
    let current_version = plugin_cfg.version.as_deref().unwrap_or("0.0.0");

    let release = ctx.client.fetch_release(&spec.github_repo(), None).await?;
    if !is_update_available(current_version, &release.tag_name) {
        return Ok(PluginUpdateStatus::AlreadyUpToDate {
            version: current_version.to_string(),
        });
    }

    let binary = download_and_extract_update(ctx.client, &release, &spec).await?;
    write_binary_atomically(&dest_path, &binary)?;
    save_updated_config(ctx, (name, plugin_cfg), &release.tag_name).await?;

    Ok(PluginUpdateStatus::Updated {
        old_version: current_version.to_string(),
        new_version: release.tag_name,
        binary_path: dest_path,
    })
}

fn print_update_status(key: &str, status: &PluginUpdateStatus) {
    match status {
        PluginUpdateStatus::AlreadyUpToDate { version } => {
            println!("Plugin '{key}' is already up to date ({version})");
        }
        PluginUpdateStatus::Updated {
            old_version,
            new_version,
            binary_path,
        } => {
            println!(
                "Updated plugin '{key}' from {old_version} to {new_version} ({})",
                binary_path.display()
            );
        }
    }
}

pub async fn handle_update_plugin(config: &Config, name: &str) -> rho_harness_core::error::Result<PluginUpdateStatus> {
    let key = resolve_plugin_key(name, &config.plugins)
        .ok_or_else(|| AppError::Plugin(format!("plugin '{name}' is not configured")))?;
    let cargo_bin =
        resolve_cargo_bin_dir().ok_or_else(|| AppError::Plugin("failed to resolve cargo bin directory".to_string()))?;
    let ctx = UpdatePluginContext {
        config_dir: &config.config_dir,
        cargo_bin_dir: &cargo_bin,
        client: &GitHubClient::new(),
    };
    let status = update_single_plugin(ctx, &key, &config.plugins[&key])
        .await
        .map_err(AppError::from)?;

    print_update_status(&key, &status);
    Ok(status)
}

fn print_update_all_table(items: &[PluginUpdateItem]) {
    println!("{:<24} {:<16} {:<16} STATUS", "NAME", "PREVIOUS", "LATEST");
    println!("{:-<24} {:-<16} {:-<16} {:-<10}", "", "", "", "");
    for item in items {
        let (prev, latest, label) = match &item.status {
            PluginUpdateStatus::AlreadyUpToDate { version } => (version.as_str(), version.as_str(), "Up to date"),
            PluginUpdateStatus::Updated {
                old_version,
                new_version,
                ..
            } => (old_version.as_str(), new_version.as_str(), "Updated"),
        };
        println!("{:<24} {:<16} {:<16} {label}", item.name, prev, latest);
    }
}

async fn collect_updated_items(
    ctx: UpdatePluginContext<'_>,
    plugins: &BTreeMap<String, PluginConfig>,
) -> rho_harness_core::error::Result<Vec<PluginUpdateItem>> {
    let mut items = Vec::new();
    for (name, plugin_cfg) in plugins {
        let status = update_single_plugin(ctx, name, plugin_cfg)
            .await
            .map_err(AppError::from)?;
        items.push(PluginUpdateItem {
            name: (*name).clone(),
            status,
        });
    }
    Ok(items)
}

pub async fn handle_update_all(config: &Config) -> rho_harness_core::error::Result<Vec<PluginUpdateItem>> {
    if config.plugins.is_empty() {
        println!("No plugins configured to update.");
        return Ok(Vec::new());
    }

    let cargo_bin =
        resolve_cargo_bin_dir().ok_or_else(|| AppError::Plugin("failed to resolve cargo bin directory".to_string()))?;
    let client = GitHubClient::new();
    let ctx = UpdatePluginContext {
        config_dir: &config.config_dir,
        cargo_bin_dir: &cargo_bin,
        client: &client,
    };

    let items = collect_updated_items(ctx, &config.plugins).await?;
    print_update_all_table(&items);
    Ok(items)
}

#[cfg(test)]
#[path = "update/tests.rs"]
mod tests;
