use serde::Deserialize;
use std::sync::LazyLock;

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
    (repo_slug, tag): (&str, Option<&str>),
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
        let resp = check_github_response(req.send().await?, (repo_slug, tag)).await?;
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

#[cfg(test)]
#[path = "github/tests.rs"]
mod tests;
