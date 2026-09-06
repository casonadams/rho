use super::archive::{ArchiveError, extract_binary};
use super::atomic::write_binary_atomically;
use super::dedup::{DuplicatePluginError, PluginCandidate, validate_no_duplicates};
use super::github::{GitHubClient, GitHubError};
use super::paths::resolve_cargo_bin_dir;
use super::platform::{Platform, PlatformMatchError, match_platform_asset};
use super::spec::{PluginSpec, PluginSpecError};
use rho_harness_core::config::{Config, PluginConfig};
use rho_harness_core::error::AppError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Spec(#[from] PluginSpecError),
    #[error(transparent)]
    Duplicate(#[from] DuplicatePluginError),
    #[error("failed to resolve cargo bin directory")]
    CargoBinNotFound,
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error(transparent)]
    PlatformMatch(#[from] PlatformMatchError),
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("binary already exists at '{0}' (use --force or --replace to overwrite)")]
    BinaryAlreadyExists(PathBuf),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(#[from] AppError),
}

impl From<InstallError> for AppError {
    fn from(err: InstallError) -> Self {
        match err {
            InstallError::Io(e) => AppError::Io(e),
            InstallError::Config(e) => e,
            other => AppError::Plugin(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallResult {
    pub name: String,
    pub version: String,
    pub binary_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct InstallPluginContext<'a> {
    pub config_dir: &'a Path,
    pub plugins: &'a BTreeMap<String, PluginConfig>,
    pub cargo_bin_dir: &'a Path,
    pub force: bool,
    pub github_client: Option<&'a GitHubClient>,
}

async fn download_and_extract_plugin(
    client: &GitHubClient,
    release: &super::github::Release,
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
        .expect("matched asset must exist in release");
    let downloaded = client.download_asset(&asset.browser_download_url).await?;
    extract_binary(&asset.name, &downloaded, &spec.executable_name).map_err(Into::into)
}

fn build_installed_plugin_config(spec: &PluginSpec, release: &super::github::Release) -> PluginConfig {
    PluginConfig {
        path: PathBuf::new(),
        command: Some(spec.executable_name.clone()),
        package: Some(spec.name.clone()),
        version: Some(release.tag_name.clone()),
        git: Some(format!("https://github.com/{}", spec.github_repo())),
        tag: spec.tag.clone(),
        enabled: true,
        ..Default::default()
    }
}

async fn fetch_and_install_binary(
    ctx: InstallPluginContext<'_>,
    spec: &PluginSpec,
    dest_path: &Path,
) -> Result<super::github::Release, InstallError> {
    let default_client = GitHubClient::new();
    let client = ctx.github_client.unwrap_or(&default_client);
    let release = client.fetch_release(&spec.github_repo(), spec.tag.as_deref()).await?;
    let binary = download_and_extract_plugin(client, &release, spec).await?;
    write_binary_atomically(dest_path, &binary)?;
    Config::add_plugin_async(
        ctx.config_dir,
        &spec.name,
        build_installed_plugin_config(spec, &release),
    )
    .await?;
    Ok(release)
}

pub async fn install_plugin(ctx: InstallPluginContext<'_>, target: &str) -> Result<InstallResult, InstallError> {
    let spec = PluginSpec::parse(target)?;
    let dest_path = ctx.cargo_bin_dir.join(&spec.executable_name);

    let candidate = PluginCandidate {
        name: spec.name.clone(),
        command: spec.executable_name.clone(),
        path: dest_path.clone(),
        force: ctx.force,
    };
    validate_no_duplicates(ctx.plugins, &candidate)?;

    if !ctx.force && dest_path.exists() {
        return Err(InstallError::BinaryAlreadyExists(dest_path));
    }

    let release = fetch_and_install_binary(ctx, &spec, &dest_path).await?;
    Ok(InstallResult {
        name: spec.name,
        version: release.tag_name,
        binary_path: dest_path,
    })
}

pub async fn handle_install(
    config: &Config,
    target: &str,
    force: bool,
) -> rho_harness_core::error::Result<InstallResult> {
    let cargo_bin =
        resolve_cargo_bin_dir().ok_or_else(|| AppError::Plugin("failed to resolve cargo bin directory".to_string()))?;
    let result = install_plugin(
        InstallPluginContext {
            config_dir: &config.config_dir,
            plugins: &config.plugins,
            cargo_bin_dir: &cargo_bin,
            force,
            github_client: None,
        },
        target,
    )
    .await?;

    println!(
        "Installed plugin '{}' ({}) to {}",
        result.name,
        result.version,
        result.binary_path.display()
    );
    Ok(result)
}

#[cfg(test)]
#[path = "install/tests.rs"]
mod tests;
