use super::archive::extract_binary;
use super::atomic::write_binary_atomically;
use super::github::GitHubClient;
use super::install::InstallError;
use super::platform::{Platform, match_platform_asset};
use super::version::is_update_available;
use rho_harness_core::config::Config;
use rho_harness_core::error::AppError;
use std::path::{Path, PathBuf};

pub const RHO_GITHUB_REPO: &str = "casonadams/rho";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfUpdateStatus {
    AlreadyUpToDate {
        version: String,
    },
    Updated {
        old_version: String,
        new_version: String,
        binary_path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SelfUpdateContext<'a> {
    pub current_exe: &'a Path,
    pub current_version: &'a str,
    pub github_client: Option<&'a GitHubClient>,
}

async fn download_and_install_self_binary(
    client: &GitHubClient,
    release: &super::github::Release,
    current_exe: &Path,
) -> Result<(), InstallError> {
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
    let binary = extract_binary(&asset.name, &downloaded, "rho")?;
    write_binary_atomically(current_exe, &binary)?;
    Ok(())
}

pub async fn self_update(ctx: SelfUpdateContext<'_>) -> Result<SelfUpdateStatus, InstallError> {
    let default_client = GitHubClient::new();
    let client = ctx.github_client.unwrap_or(&default_client);
    let release = client.fetch_release(RHO_GITHUB_REPO, None).await?;

    if !is_update_available(ctx.current_version, &release.tag_name) {
        return Ok(SelfUpdateStatus::AlreadyUpToDate {
            version: ctx.current_version.to_string(),
        });
    }

    download_and_install_self_binary(client, &release, ctx.current_exe).await?;

    Ok(SelfUpdateStatus::Updated {
        old_version: ctx.current_version.to_string(),
        new_version: release.tag_name,
        binary_path: ctx.current_exe.to_path_buf(),
    })
}

fn print_self_update_status(status: &SelfUpdateStatus) {
    match status {
        SelfUpdateStatus::AlreadyUpToDate { version } => {
            println!("rho is already up to date ({version})");
        }
        SelfUpdateStatus::Updated {
            old_version,
            new_version,
            binary_path,
        } => {
            println!(
                "Updated rho from {old_version} to {new_version} ({})",
                binary_path.display()
            );
        }
    }
}

pub async fn handle_self_update(_config: &Config) -> rho_harness_core::error::Result<SelfUpdateStatus> {
    let current_exe = std::env::current_exe()?;
    let status = self_update(SelfUpdateContext {
        current_exe: &current_exe,
        current_version: env!("CARGO_PKG_VERSION"),
        github_client: None,
    })
    .await
    .map_err(AppError::from)?;

    print_self_update_status(&status);
    Ok(status)
}

#[cfg(test)]
#[path = "self_update/tests.rs"]
mod tests;
