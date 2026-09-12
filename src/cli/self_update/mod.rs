pub mod archive;
pub mod platform;

use archive::{extract_binary, write_binary_atomically};
use platform::{Platform, match_platform_asset};
use rho_harness_core::config::Config;
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const RHO_GITHUB_REPO: &str = "casonadams/rho";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("failed to initialize reqwest client")
});

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("release not found for {repo_slug}{tag_info}")]
    NotFound { repo_slug: String, tag_info: String },
    #[error("GitHub API rate limit exceeded; set GITHUB_TOKEN environment variable")]
    RateLimited,
    #[error("GitHub API error: {0}")]
    Api(String),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to parse GitHub response: {0}")]
    Parse(String),
}

#[derive(Clone, Debug)]
pub struct GitHubClient {
    base_url: String,
    client: reqwest::Client,
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubClient {
    pub fn new() -> Self {
        Self {
            base_url: "https://api.github.com".to_string(),
            client: CLIENT.clone(),
        }
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: CLIENT.clone(),
        }
    }

    pub async fn fetch_release(&self, repo_slug: &str, tag: Option<&str>) -> std::result::Result<Release, GitHubError> {
        let url = match tag {
            Some(t) => format!("{}/repos/{repo_slug}/releases/tags/{t}", self.base_url),
            None => format!("{}/repos/{repo_slug}/releases/latest", self.base_url),
        };
        let mut req = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                format!("rho/{}", env!("CARGO_PKG_VERSION")),
            )
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
            req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(GitHubError::NotFound {
                repo_slug: repo_slug.to_string(),
                tag_info: tag.map(|t| format!(" @ {t}")).unwrap_or_default(),
            });
        }
        if status == reqwest::StatusCode::FORBIDDEN
            && resp.headers().get("x-ratelimit-remaining").is_some_and(|r| r == "0")
        {
            return Err(GitHubError::RateLimited);
        }
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api(format!("status {status}: {error_text}")));
        }
        resp.json::<Release>()
            .await
            .map_err(|e| GitHubError::Parse(e.to_string()))
    }

    pub async fn download_asset(&self, url: &str) -> std::result::Result<Vec<u8>, GitHubError> {
        let resp = self
            .client
            .get(url)
            .header(
                reqwest::header::USER_AGENT,
                format!("rho/{}", env!("CARGO_PKG_VERSION")),
            )
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(GitHubError::Api(format!(
                "download failed with status {}",
                resp.status()
            )));
        }
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
}

impl SimpleVersion {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let (version_part, prerelease) = match trimmed.split_once('-') {
            Some((v, p)) => (v, Some(p.to_string())),
            None => (trimmed, None),
        };
        let mut parts = version_part.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    pub fn is_newer_than(&self, other: &Self) -> bool {
        let self_triple = (self.major, self.minor, self.patch);
        let other_triple = (other.major, other.minor, other.patch);
        if self_triple != other_triple {
            return self_triple > other_triple;
        }
        other.prerelease.is_some() && self.prerelease.is_none()
    }
}

pub fn is_update_available(current: &str, latest: &str) -> bool {
    let cur_trimmed = current.trim();
    let latest_trimmed = latest.trim();
    if cur_trimmed == latest_trimmed {
        return false;
    }
    match (SimpleVersion::parse(cur_trimmed), SimpleVersion::parse(latest_trimmed)) {
        (Some(c), Some(l)) => l.is_newer_than(&c),
        _ => cur_trimmed != latest_trimmed,
    }
}

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

async fn download_and_install_self_binary(client: &GitHubClient, release: &Release, current_exe: &Path) -> Result<()> {
    let platform = Platform::current().ok_or_else(|| {
        AppError::Other(anyhow::anyhow!(
            "Unsupported platform: {}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ))
    })?;
    let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
    let matched_name =
        match_platform_asset(&platform, &asset_names).map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == matched_name)
        .expect("matched asset must exist in release");
    let downloaded = client
        .download_asset(&asset.browser_download_url)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
    let binary =
        extract_binary(&asset.name, &downloaded, "rho").map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
    write_binary_atomically(current_exe, &binary).map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;
    Ok(())
}

pub async fn self_update(ctx: SelfUpdateContext<'_>) -> Result<SelfUpdateStatus> {
    let default_client = GitHubClient::new();
    let client = ctx.github_client.unwrap_or(&default_client);
    let release = client
        .fetch_release(RHO_GITHUB_REPO, None)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!(e.to_string())))?;

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

pub async fn handle_self_update(_config: &Config) -> Result<SelfUpdateStatus> {
    let current_exe = std::env::current_exe()?;
    let status = self_update(SelfUpdateContext {
        current_exe: &current_exe,
        current_version: env!("CARGO_PKG_VERSION"),
        github_client: None,
    })
    .await?;

    print_self_update_status(&status);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn create_tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut tar = tar::Builder::new(&mut gz);
            for (name, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_path(name).unwrap();
                header.set_size(content.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append(&header, *content).unwrap();
            }
            tar.finish().unwrap();
        }
        gz.finish().unwrap()
    }

    async fn serve_raw_json(mut stream: tokio::net::TcpStream, body: &str) {
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    }

    async fn serve_raw_bytes(mut stream: tokio::net::TcpStream, bytes: &[u8]) {
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.write_all(bytes).await;
    }

    #[tokio::test]
    async fn test_self_update_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let platform = Platform::current().unwrap();
        let asset_name = format!("rho-v0.4.0-{}.tar.gz", platform.target_triple());
        let fake_tar = create_tar_gz(&[("rho", b"#!/bin/sh\necho updated\n")]);
        let fake_tar_arc = Arc::new(fake_tar);

        let body = format!(
            r#"{{"tag_name":"v0.4.0","assets":[{{"name":"{asset_name}","browser_download_url":"http://{addr}/download/{asset_name}"}}]}}"#
        );

        let fake_tar_clone = Arc::clone(&fake_tar_arc);
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                serve_raw_json(stream, &body).await;
            }
            if let Ok((stream, _)) = listener.accept().await {
                serve_raw_bytes(stream, &fake_tar_clone).await;
            }
        });

        let temp = tempfile::tempdir().unwrap();
        let current_exe = temp.path().join("rho");
        std::fs::write(&current_exe, b"#!/bin/sh\necho old\n").unwrap();

        let client = GitHubClient::with_base_url(format!("http://{addr}"));
        let status = self_update(SelfUpdateContext {
            current_exe: &current_exe,
            current_version: "v0.3.0",
            github_client: Some(&client),
        })
        .await
        .unwrap();

        assert!(matches!(status, SelfUpdateStatus::Updated { .. }));
        let content = std::fs::read(&current_exe).unwrap();
        assert_eq!(content, b"#!/bin/sh\necho updated\n");
    }
}
