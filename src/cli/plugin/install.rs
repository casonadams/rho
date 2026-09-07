//! Plugin installation flow and GitHub release assets client.

use super::archive::{ArchiveError, extract_binary, write_binary_atomically};
use super::paths::resolve_cargo_bin_dir;
use super::platform::{Platform, PlatformMatchError, match_platform_asset};
use super::spec::{DuplicatePluginError, PluginCandidate, PluginSpec, PluginSpecError, validate_no_duplicates};
use rho_harness_core::config::{Config, PluginConfig};
use rho_harness_core::error::AppError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// GitHub Release API client
// ---------------------------------------------------------------------------

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    rho_engine::install_crypto_provider();
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

fn build_github_request(client: &reqwest::Client, url: &str) -> reqwest::RequestBuilder {
    let mut req = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            format!("rho/{}", env!("CARGO_PKG_VERSION")),
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    req
}

async fn check_github_response(
    resp: reqwest::Response,
    repo_slug: &str,
    tag: Option<&str>,
) -> Result<reqwest::Response, GitHubError> {
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(GitHubError::NotFound {
            repo_slug: repo_slug.to_string(),
            tag_info: tag.map(|t| format!(" @ {t}")).unwrap_or_default(),
        });
    }
    if status == reqwest::StatusCode::FORBIDDEN
        && resp
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|rem| rem == "0")
    {
        return Err(GitHubError::RateLimited);
    }
    if !status.is_success() {
        let error_text = resp.text().await.unwrap_or_default();
        return Err(GitHubError::Api(format!("status {status}: {error_text}")));
    }
    Ok(resp)
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

    pub async fn fetch_release(&self, repo_slug: &str, tag: Option<&str>) -> Result<Release, GitHubError> {
        let url = match tag {
            Some(t) => format!("{}/repos/{repo_slug}/releases/tags/{t}", self.base_url),
            None => format!("{}/repos/{repo_slug}/releases/latest", self.base_url),
        };
        let req = build_github_request(&self.client, &url);
        let resp = check_github_response(req.send().await?, repo_slug, tag).await?;
        resp.json::<Release>()
            .await
            .map_err(|e| GitHubError::Parse(e.to_string()))
    }

    pub async fn download_asset(&self, url: &str) -> Result<Vec<u8>, GitHubError> {
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

// ---------------------------------------------------------------------------
// Installation flow
// ---------------------------------------------------------------------------

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
    release: &Release,
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

fn build_installed_plugin_config(spec: &PluginSpec, release: &Release) -> PluginConfig {
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
) -> Result<Release, InstallError> {
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
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_fetch_latest_release_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"tag_name":"v1.0.0","assets":[{"name":"plugin-mac.tar.gz","browser_download_url":"http://example.com/asset"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = GitHubClient::with_base_url(format!("http://{addr}"));
        let release = client.fetch_release("casonadams/my-plugin", None).await.unwrap();

        assert_eq!(release.tag_name, "v1.0.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "plugin-mac.tar.gz");
    }

    #[tokio::test]
    async fn test_fetch_tagged_release_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.contains("/releases/tags/v2.1.0"));
            let body = r#"{"tag_name":"v2.1.0","assets":[]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = GitHubClient::with_base_url(format!("http://{addr}"));
        let release = client
            .fetch_release("casonadams/my-plugin", Some("v2.1.0"))
            .await
            .unwrap();

        assert_eq!(release.tag_name, "v2.1.0");
    }

    #[tokio::test]
    async fn test_fetch_release_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"message":"Not Found"}"#;
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = GitHubClient::with_base_url(format!("http://{addr}"));
        let result = client.fetch_release("casonadams/missing", None).await;

        match result {
            Err(GitHubError::NotFound { repo_slug, .. }) => {
                assert_eq!(repo_slug, "casonadams/missing");
            }
            other => panic!("expected NotFound error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_fetch_release_rate_limited() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"message":"API rate limit exceeded"}"#;
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = GitHubClient::with_base_url(format!("http://{addr}"));
        let result = client.fetch_release("casonadams/limited", None).await;

        assert!(matches!(result, Err(GitHubError::RateLimited)));
    }

    #[tokio::test]
    async fn test_download_asset_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let payload = b"hello binary data";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(payload).await.unwrap();
        });

        let client = GitHubClient::new();
        let data = client
            .download_asset(&format!("http://{addr}/asset.tar.gz"))
            .await
            .unwrap();
        assert_eq!(data, b"hello binary data");
    }

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

    async fn serve_raw_json(mut stream: tokio::net::TcpStream, body: &str) {
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    }

    async fn spawn_install_download_server(asset_name: String, asset: std::sync::Arc<Vec<u8>>) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = format!(
            r#"{{"tag_name":"v1.2.0","assets":[{{"name":"{asset_name}","browser_download_url":"http://{addr}/download/{asset_name}"}}]}}"#
        );
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                serve_raw_json(stream, &body).await;
            }
            if let Ok((stream, _)) = listener.accept().await {
                serve_raw_bytes(stream, &asset).await;
            }
        });
        addr
    }

    async fn spawn_single_json_server(body: &'static str) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn test_install_plugin_success_tar_gz() {
        let triple = Platform::current().unwrap().target_triple();
        let asset_name = format!("rho-plugin-sample-{triple}.tar.gz");
        let asset = std::sync::Arc::new(create_tar_gz(&[("rho-plugin-sample", b"sample-binary-payload")]));
        let addr = spawn_install_download_server(asset_name, asset).await;

        let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let plugins = BTreeMap::new();
        let github_client = GitHubClient::with_base_url(format!("http://{addr}"));
        let ctx = InstallPluginContext {
            config_dir: config_dir.path(),
            plugins: &plugins,
            cargo_bin_dir: bin_dir.path(),
            force: false,
            github_client: Some(&github_client),
        };
        let result = install_plugin(ctx, "sample").await.unwrap();

        assert_eq!(
            (result.name.as_str(), result.version.as_str()),
            ("rho-plugin-sample", "v1.2.0")
        );
        assert_eq!(result.binary_path, bin_dir.path().join("rho-plugin-sample"));
        assert_eq!(std::fs::read(&result.binary_path).unwrap(), b"sample-binary-payload");
    }

    #[tokio::test]
    async fn test_install_duplicate_plugin_fails_without_force() {
        let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let plugins = BTreeMap::from([(
            "rho-plugin-sample".to_string(),
            PluginConfig {
                command: Some("rho-plugin-sample".to_string()),
                ..Default::default()
            },
        )]);
        let ctx = InstallPluginContext {
            config_dir: config_dir.path(),
            plugins: &plugins,
            cargo_bin_dir: bin_dir.path(),
            force: false,
            github_client: None,
        };
        assert!(matches!(
            install_plugin(ctx, "sample").await.unwrap_err(),
            InstallError::Duplicate(_)
        ));
    }

    #[tokio::test]
    async fn test_install_existing_binary_fails_without_force() {
        let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let binary_file = bin_dir.path().join("rho-plugin-sample");
        std::fs::write(&binary_file, b"existing").unwrap();

        let plugins = BTreeMap::new();
        let ctx = InstallPluginContext {
            config_dir: config_dir.path(),
            plugins: &plugins,
            cargo_bin_dir: bin_dir.path(),
            force: false,
            github_client: None,
        };
        match install_plugin(ctx, "sample").await.unwrap_err() {
            InstallError::BinaryAlreadyExists(path) => assert_eq!(path, binary_file),
            other => panic!("expected BinaryAlreadyExists, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_install_no_matching_asset_fails() {
        let body = r#"{"tag_name":"v1.0.0","assets":[{"name":"incompatible-platform.deb","browser_download_url":"http://example.com/asset"}]}"#;
        let addr = spawn_single_json_server(body).await;

        let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let plugins = BTreeMap::new();
        let github_client = GitHubClient::with_base_url(format!("http://{addr}"));
        let ctx = InstallPluginContext {
            config_dir: config_dir.path(),
            plugins: &plugins,
            cargo_bin_dir: bin_dir.path(),
            force: false,
            github_client: Some(&github_client),
        };
        assert!(matches!(
            install_plugin(ctx, "sample").await.unwrap_err(),
            InstallError::PlatformMatch(_)
        ));
    }
}
