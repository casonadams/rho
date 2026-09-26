//! Specialized extractor for GitHub resources (issues, PRs, commits, blobs, trees).

use crate::tools::web::http::{HttpClient, HttpRequest};
use rho_harness_core::error::AppError;
use std::time::Duration;
use url::Url;

/// Represents a recognized GitHub URL target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubUrl {
    Issue {
        owner: String,
        repo: String,
        number: u64,
    },
    PullRequest {
        owner: String,
        repo: String,
        number: u64,
    },
    Commit {
        owner: String,
        repo: String,
        sha: String,
    },
    Blob {
        owner: String,
        repo: String,
        ref_name: String,
        path: String,
    },
    Tree {
        owner: String,
        repo: String,
        ref_name: Option<String>,
        path: String,
    },
    Repo {
        owner: String,
        repo: String,
    },
}

impl GitHubUrl {
    /// Convert a blob URL target into a direct raw.githubusercontent.com URL.
    pub fn to_raw_url(&self) -> Option<String> {
        match self {
            Self::Blob {
                owner,
                repo,
                ref_name,
                path,
            } => Some(format!(
                "https://raw.githubusercontent.com/{owner}/{repo}/{ref_name}/{path}"
            )),
            _ => None,
        }
    }
}

fn parse_issue_or_pr(owner: &str, repo: &str, section: &str, remaining: &[&str]) -> Option<GitHubUrl> {
    if remaining.len() != 1 {
        return None;
    }
    let number = remaining[0].parse::<u64>().ok()?;
    match section {
        "issues" => Some(GitHubUrl::Issue {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
        }),
        "pull" | "pulls" => Some(GitHubUrl::PullRequest {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
        }),
        _ => None,
    }
}

fn parse_commit(owner: &str, repo: &str, remaining: &[&str]) -> Option<GitHubUrl> {
    if remaining.len() == 1 && !remaining[0].is_empty() {
        Some(GitHubUrl::Commit {
            owner: owner.to_string(),
            repo: repo.to_string(),
            sha: remaining[0].to_string(),
        })
    } else {
        None
    }
}

fn parse_blob_or_tree(owner: &str, repo: &str, section: &str, remaining: &[&str]) -> Option<GitHubUrl> {
    if section == "blob" {
        if remaining.is_empty() {
            return None;
        }
        let ref_name = remaining[0].to_string();
        let path = remaining[1..].join("/");
        return Some(GitHubUrl::Blob {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_name,
            path,
        });
    }
    if section == "tree" {
        let ref_name = remaining.first().map(|s| s.to_string());
        let path = if remaining.len() > 1 {
            remaining[1..].join("/")
        } else {
            String::new()
        };
        return Some(GitHubUrl::Tree {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_name,
            path,
        });
    }
    None
}

/// Parse a URL into a structured `GitHubUrl` if it targets github.com.
pub fn parse_github_url(raw_url: &str) -> Option<GitHubUrl> {
    let parsed = Url::parse(raw_url).ok()?;
    let host = parsed.host_str()?;
    if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
        return None;
    }

    let mut segments = parsed.path_segments()?;
    let owner = segments.next()?.trim();
    let repo_raw = segments.next()?.trim();
    let repo = repo_raw.strip_suffix(".git").unwrap_or(repo_raw);

    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    let section = match segments.next() {
        Some(s) => s,
        None => {
            return Some(GitHubUrl::Repo {
                owner: owner.to_string(),
                repo: repo.to_string(),
            });
        }
    };

    let remaining: Vec<&str> = segments.collect();
    match section {
        "issues" | "pull" | "pulls" => parse_issue_or_pr(owner, repo, section, &remaining),
        "commit" => parse_commit(owner, repo, &remaining),
        "blob" | "tree" => parse_blob_or_tree(owner, repo, section, &remaining),
        _ => None,
    }
}

/// Retrieve GitHub personal access token from environment.
pub fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|t| !t.trim().is_empty())
}

fn check_github_status(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    path_or_url: &str,
) -> Result<(), AppError> {
    if status == reqwest::StatusCode::FORBIDDEN
        && headers.get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0")
    {
        return Err(AppError::Tool(
            "GitHub API rate limit exceeded. Set GITHUB_TOKEN environment variable for higher rate limits.".to_string(),
        ));
    }

    if !status.is_success() {
        return Err(AppError::Tool(format!("GitHub API error {status} for {path_or_url}")));
    }

    Ok(())
}

/// Perform an authenticated GET request against a full URL.
pub async fn fetch_github_api_url(
    client: &reqwest::Client,
    url: &str,
    timeout_sec: u64,
    accept: Option<&str>,
) -> Result<String, AppError> {
    let mut req = client
        .get(url)
        .header("User-Agent", "rho-agent")
        .header("Accept", accept.unwrap_or("application/vnd.github.v3+json"))
        .timeout(Duration::from_secs(timeout_sec));

    if let Some(token) = github_token() {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Tool(format!("GitHub API request failed: {e}")))?;

    check_github_status(resp.status(), resp.headers(), url)?;

    resp.text()
        .await
        .map_err(|e| AppError::Tool(format!("Failed to read GitHub API response: {e}")))
}

/// Perform an authenticated GET request against the GitHub REST API.
pub async fn fetch_github_api(
    client: &reqwest::Client,
    path: &str,
    timeout_sec: u64,
    accept: Option<&str>,
) -> Result<String, AppError> {
    let url = format!("https://api.github.com{path}");
    fetch_github_api_url(client, &url, timeout_sec, accept).await
}

/// Format an issue and its comments into clean Markdown.
pub fn format_issue(issue: &serde_json::Value, comments: &[serde_json::Value]) -> String {
    let title = issue["title"].as_str().unwrap_or("Untitled Issue");
    let number = issue["number"].as_u64().unwrap_or(0);
    let state = issue["state"].as_str().unwrap_or("unknown");
    let author = issue["user"]["login"].as_str().unwrap_or("unknown");
    let created_at = issue["created_at"].as_str().unwrap_or("");
    let updated_at = issue["updated_at"].as_str().unwrap_or("");
    let body = issue["body"].as_str().unwrap_or("*No description provided.*");

    let labels: Vec<&str> = issue["labels"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|l| l["name"].as_str()).collect())
        .unwrap_or_default();

    let mut out = format!("# {title}\n\n");
    out.push_str(&format!("**#{number}** · {state} · opened by @{author}\n"));
    if !created_at.is_empty() {
        out.push_str(&format!("Created: {created_at}"));
        if !updated_at.is_empty() && updated_at != created_at {
            out.push_str(&format!(" · Updated: {updated_at}"));
        }
        out.push('\n');
    }
    if !labels.is_empty() {
        out.push_str(&format!("Labels: {}\n", labels.join(", ")));
    }
    out.push_str("\n---\n\n");
    out.push_str(body);
    out.push_str("\n\n");

    if !comments.is_empty() {
        out.push_str(&format!("---\n\n## Comments ({})\n\n", comments.len()));
        for comment in comments {
            let c_author = comment["user"]["login"].as_str().unwrap_or("unknown");
            let c_created = comment["created_at"].as_str().unwrap_or("");
            let c_body = comment["body"].as_str().unwrap_or("");
            out.push_str(&format!("### @{c_author} · {c_created}\n\n"));
            out.push_str(c_body);
            out.push_str("\n\n---\n\n");
        }
    }

    out.trim_end().to_string()
}

/// Format a pull request, its comments, and diff into clean Markdown.
pub fn format_pull_request(pr: &serde_json::Value, comments: &[serde_json::Value], diff: Option<&str>) -> String {
    let title = pr["title"].as_str().unwrap_or("Untitled PR");
    let number = pr["number"].as_u64().unwrap_or(0);
    let state = if pr["merged_at"].is_string() {
        "merged"
    } else {
        pr["state"].as_str().unwrap_or("unknown")
    };
    let author = pr["user"]["login"].as_str().unwrap_or("unknown");
    let created_at = pr["created_at"].as_str().unwrap_or("");
    let base = pr["base"]["ref"].as_str().unwrap_or("main");
    let head = pr["head"]["ref"].as_str().unwrap_or("branch");
    let additions = pr["additions"].as_u64().unwrap_or(0);
    let deletions = pr["deletions"].as_u64().unwrap_or(0);
    let changed_files = pr["changed_files"].as_u64().unwrap_or(0);
    let body = pr["body"].as_str().unwrap_or("*No description provided.*");

    let mut out = format!("# {title}\n\n");
    out.push_str(&format!("**#{number}** · {state} · opened by @{author}\n"));
    out.push_str(&format!("Branches: `{base}` ← `{head}`\n"));
    out.push_str(&format!("{changed_files} files changed · +{additions} −{deletions}\n"));
    if !created_at.is_empty() {
        out.push_str(&format!("Created: {created_at}\n"));
    }
    out.push_str("\n---\n\n");
    out.push_str(body);
    out.push_str("\n\n");

    if !comments.is_empty() {
        out.push_str(&format!("---\n\n## Comments ({})\n\n", comments.len()));
        for comment in comments {
            let c_author = comment["user"]["login"].as_str().unwrap_or("unknown");
            let c_created = comment["created_at"].as_str().unwrap_or("");
            let c_body = comment["body"].as_str().unwrap_or("");
            out.push_str(&format!("### @{c_author} · {c_created}\n\n"));
            out.push_str(c_body);
            out.push_str("\n\n---\n\n");
        }
    }

    if let Some(diff_text) = diff
        && !diff_text.trim().is_empty()
    {
        out.push_str("---\n\n## Diff\n\n```diff\n");
        let lines: Vec<&str> = diff_text.lines().collect();
        if lines.len() > 1000 {
            out.push_str(&lines[..1000].join("\n"));
            out.push_str(&format!(
                "\n\n[Diff truncated: showing first 1000 lines of {} total lines]\n",
                lines.len()
            ));
        } else {
            out.push_str(diff_text);
            if !diff_text.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push_str("```\n");
    }

    out.trim_end().to_string()
}

fn format_commit_parents(parents: &[serde_json::Value]) -> Option<String> {
    if parents.is_empty() {
        return None;
    }
    let parent_shas: Vec<&str> = parents
        .iter()
        .filter_map(|p| p["sha"].as_str().map(|s| if s.len() >= 12 { &s[..12] } else { s }))
        .collect();
    Some(format!("Parents: {}\n", parent_shas.join(", ")))
}

fn format_commit_files(file_list: &[serde_json::Value]) -> String {
    let mut out = format!("\n---\n\n## Files ({})\n\n", file_list.len());
    for file in file_list {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        let status = file["status"].as_str().unwrap_or("modified");
        let f_add = file["additions"].as_u64().unwrap_or(0);
        let f_del = file["deletions"].as_u64().unwrap_or(0);

        out.push_str(&format!("### {filename}\n\n"));
        out.push_str(&format!("{status} · +{f_add} −{f_del}\n\n"));

        if let Some(patch) = file["patch"].as_str() {
            out.push_str("```diff\n");
            out.push_str(patch);
            if !patch.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        } else {
            out.push_str("*No textual diff (binary or unchanged).*\n\n");
        }
    }
    out
}

/// Format a commit, its stats, and file diff patches into clean Markdown.
pub fn format_commit(commit: &serde_json::Value) -> String {
    let sha = commit["sha"].as_str().unwrap_or("unknown");
    let short_sha = if sha.len() >= 12 { &sha[..12] } else { sha };
    let message = commit["commit"]["message"].as_str().unwrap_or("");
    let mut msg_lines = message.lines();
    let subject = msg_lines.next().unwrap_or("");
    let body: String = msg_lines.collect::<Vec<&str>>().join("\n").trim().to_string();

    let author_login = commit["author"]["login"].as_str();
    let author_name = commit["commit"]["author"]["name"].as_str().unwrap_or("unknown");
    let author_display = match author_login {
        Some(login) => format!("@{login}"),
        None => author_name.to_string(),
    };
    let date = commit["commit"]["author"]["date"].as_str().unwrap_or("");

    let mut out = format!("# {subject}\n\n");
    out.push_str(&format!("**{short_sha}** · authored by {author_display}"));
    if !date.is_empty() {
        out.push_str(&format!(" · {date}"));
    }
    out.push('\n');

    let additions = commit["stats"]["additions"].as_u64().unwrap_or(0);
    let deletions = commit["stats"]["deletions"].as_u64().unwrap_or(0);
    let files = commit["files"].as_array();
    let file_count = files.map(|f| f.len()).unwrap_or(0);
    out.push_str(&format!("{file_count} files changed · +{additions} −{deletions}\n"));

    if let Some(parents) = commit["parents"].as_array()
        && let Some(parents_str) = format_commit_parents(parents)
    {
        out.push_str(&parents_str);
    }

    if !body.is_empty() {
        out.push_str(&format!("\n{body}\n"));
    }

    if let Some(file_list) = files
        && !file_list.is_empty()
    {
        out.push_str(&format_commit_files(file_list));
    }

    out.trim_end().to_string()
}

/// Format a directory tree listing into Markdown table.
pub fn format_tree(
    entries: &[serde_json::Value],
    owner: &str,
    repo: &str,
    ref_name: Option<&str>,
    path: &str,
) -> String {
    let display_path = if path.is_empty() { "(root)" } else { path };
    let mut out = format!("# {owner}/{repo}/{display_path}\n\n");
    if let Some(r) = ref_name {
        out.push_str(&format!("**Branch/Ref:** `{r}`\n\n"));
    }

    out.push_str("| Type | Name | Size |\n");
    out.push_str("|---|---|---|\n");

    for entry in entries {
        let name = entry["name"].as_str().unwrap_or("");
        let entry_type = entry["type"].as_str().unwrap_or("file");
        let size = entry["size"].as_u64();
        let (icon, size_str) = match entry_type {
            "dir" => ("📁", "-".to_string()),
            "file" => ("📄", size.map(|s| format!("{s} B")).unwrap_or_else(|| "-".to_string())),
            "submodule" => ("📦", "submodule".to_string()),
            "symlink" => ("🔗", "symlink".to_string()),
            _ => ("❓", "-".to_string()),
        };
        out.push_str(&format!("| {icon} {entry_type} | `{name}` | {size_str} |\n"));
    }

    out.trim_end().to_string()
}

/// Format repository overview metadata into Markdown.
pub fn format_repo(repo: &serde_json::Value) -> String {
    let full_name = repo["full_name"].as_str().unwrap_or("unknown");
    let description = repo["description"].as_str().unwrap_or("*No description provided.*");
    let default_branch = repo["default_branch"].as_str().unwrap_or("main");
    let stars = repo["stargazers_count"].as_u64().unwrap_or(0);
    let forks = repo["forks_count"].as_u64().unwrap_or(0);
    let open_issues = repo["open_issues_count"].as_u64().unwrap_or(0);

    let mut out = format!("# {full_name}\n\n");
    out.push_str(&format!("{description}\n\n"));
    out.push_str(&format!("- **Default branch:** `{default_branch}`\n"));
    out.push_str(&format!("- **Stars:** {stars}\n"));
    out.push_str(&format!("- **Forks:** {forks}\n"));
    out.push_str(&format!("- **Open issues:** {open_issues}\n"));

    out.trim_end().to_string()
}

async fn extract_issue(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    number: u64,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let raw = fetch_github_api(
        client,
        &format!("/repos/{owner}/{repo}/issues/{number}"),
        timeout_sec,
        None,
    )
    .await?;
    let issue_json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Tool(format!("Failed to parse issue JSON: {e}")))?;

    let comments_count = issue_json["comments"].as_u64().unwrap_or(0);
    let comments: Vec<serde_json::Value> = if comments_count > 0 {
        let comments_raw = fetch_github_api(
            client,
            &format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"),
            timeout_sec,
            None,
        )
        .await
        .unwrap_or_default();
        serde_json::from_str(&comments_raw).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(format_issue(&issue_json, &comments))
}

async fn extract_pull_request(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    number: u64,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let raw = fetch_github_api(
        client,
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        timeout_sec,
        None,
    )
    .await?;
    let pr_json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Tool(format!("Failed to parse PR JSON: {e}")))?;

    let comments_count = pr_json["comments"].as_u64().unwrap_or(0);
    let comments: Vec<serde_json::Value> = if comments_count > 0 {
        let comments_raw = fetch_github_api(
            client,
            &format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"),
            timeout_sec,
            None,
        )
        .await
        .unwrap_or_default();
        serde_json::from_str(&comments_raw).unwrap_or_default()
    } else {
        Vec::new()
    };

    let diff_text = fetch_github_api(
        client,
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        timeout_sec,
        Some("application/vnd.github.v3.diff"),
    )
    .await
    .ok();

    Ok(format_pull_request(&pr_json, &comments, diff_text.as_deref()))
}

async fn extract_commit(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    sha: &str,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let raw = fetch_github_api(
        client,
        &format!("/repos/{owner}/{repo}/commits/{sha}"),
        timeout_sec,
        None,
    )
    .await?;
    let commit_json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Tool(format!("Failed to parse commit JSON: {e}")))?;

    Ok(format_commit(&commit_json))
}

async fn extract_tree(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    ref_name: Option<&str>,
    path: &str,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let api_path = match ref_name {
        Some(r) => format!("/repos/{owner}/{repo}/contents/{path}?ref={r}"),
        None => format!("/repos/{owner}/{repo}/contents/{path}"),
    };
    let raw = fetch_github_api(client, &api_path, timeout_sec, None).await?;
    let tree_json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Tool(format!("Failed to parse tree contents JSON: {e}")))?;

    let entries = tree_json.as_array().cloned().unwrap_or_default();
    Ok(format_tree(&entries, owner, repo, ref_name, path))
}

async fn extract_blob(
    http: &HttpClient,
    owner: &str,
    repo: &str,
    ref_name: &str,
    path: &str,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let raw_url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{ref_name}/{path}");
    let req = HttpRequest {
        url: &raw_url,
        user_agent: Some("rho-agent"),
        timeout_sec,
        max_bytes: 10 * 1024 * 1024,
        pdf_max_bytes: None,
    };
    let resp = http.get_text(req).await?;
    Ok(resp.body)
}

async fn extract_repo(client: &reqwest::Client, owner: &str, repo: &str, timeout_sec: u64) -> Result<String, AppError> {
    let raw = fetch_github_api(client, &format!("/repos/{owner}/{repo}"), timeout_sec, None).await?;
    let repo_json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| AppError::Tool(format!("Failed to parse repo JSON: {e}")))?;

    Ok(format_repo(&repo_json))
}

async fn dispatch_extract_activity(
    client: &reqwest::Client,
    target: &GitHubUrl,
    timeout_sec: u64,
) -> Result<String, AppError> {
    match target {
        GitHubUrl::Issue { owner, repo, number } => extract_issue(client, owner, repo, *number, timeout_sec).await,
        GitHubUrl::PullRequest { owner, repo, number } => {
            extract_pull_request(client, owner, repo, *number, timeout_sec).await
        }
        _ => Err(AppError::Tool("Invalid activity target".to_string())),
    }
}

async fn dispatch_extract_version(
    client: &reqwest::Client,
    target: &GitHubUrl,
    timeout_sec: u64,
) -> Result<String, AppError> {
    match target {
        GitHubUrl::Commit { owner, repo, sha } => extract_commit(client, owner, repo, sha, timeout_sec).await,
        GitHubUrl::Repo { owner, repo } => extract_repo(client, owner, repo, timeout_sec).await,
        _ => Err(AppError::Tool("Invalid version target".to_string())),
    }
}

async fn dispatch_extract_content(http: &HttpClient, target: &GitHubUrl, timeout_sec: u64) -> Result<String, AppError> {
    match target {
        GitHubUrl::Blob {
            owner,
            repo,
            ref_name,
            path,
        } => extract_blob(http, owner, repo, ref_name, path, timeout_sec).await,
        GitHubUrl::Tree {
            owner,
            repo,
            ref_name,
            path,
        } => extract_tree(&http.client, owner, repo, ref_name.as_deref(), path, timeout_sec).await,
        _ => Err(AppError::Tool("Invalid content target".to_string())),
    }
}

/// Main entry point to extract GitHub resource content from a target URL.
pub async fn extract_github(http: &HttpClient, target: &GitHubUrl, timeout_sec: u64) -> Result<String, AppError> {
    match target {
        GitHubUrl::Issue { .. } | GitHubUrl::PullRequest { .. } => {
            dispatch_extract_activity(&http.client, target, timeout_sec).await
        }
        GitHubUrl::Commit { .. } | GitHubUrl::Repo { .. } => {
            dispatch_extract_version(&http.client, target, timeout_sec).await
        }
        GitHubUrl::Blob { .. } | GitHubUrl::Tree { .. } => dispatch_extract_content(http, target, timeout_sec).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_issue_url() {
        let url = "https://github.com/casonadams/rho/issues/38";
        assert_eq!(
            parse_github_url(url),
            Some(GitHubUrl::Issue {
                owner: "casonadams".into(),
                repo: "rho".into(),
                number: 38,
            })
        );
    }

    #[test]
    fn parse_pull_request_url() {
        let url = "https://github.com/casonadams/rho/pull/42";
        assert_eq!(
            parse_github_url(url),
            Some(GitHubUrl::PullRequest {
                owner: "casonadams".into(),
                repo: "rho".into(),
                number: 42,
            })
        );
    }

    #[test]
    fn parse_commit_url() {
        let url = "https://github.com/casonadams/rho/commit/a1b2c3d4e5f6";
        assert_eq!(
            parse_github_url(url),
            Some(GitHubUrl::Commit {
                owner: "casonadams".into(),
                repo: "rho".into(),
                sha: "a1b2c3d4e5f6".into(),
            })
        );
    }

    #[test]
    fn parse_blob_url_and_to_raw() {
        let url = "https://github.com/casonadams/rho/blob/main/crates/rho-engine/Cargo.toml";
        let parsed = parse_github_url(url).expect("parsed blob url");
        assert_eq!(
            parsed,
            GitHubUrl::Blob {
                owner: "casonadams".into(),
                repo: "rho".into(),
                ref_name: "main".into(),
                path: "crates/rho-engine/Cargo.toml".into(),
            }
        );
        assert_eq!(
            parsed.to_raw_url().as_deref(),
            Some("https://raw.githubusercontent.com/casonadams/rho/main/crates/rho-engine/Cargo.toml")
        );
    }

    #[test]
    fn parse_tree_url() {
        let url = "https://github.com/casonadams/rho/tree/main/crates/rho-engine";
        assert_eq!(
            parse_github_url(url),
            Some(GitHubUrl::Tree {
                owner: "casonadams".into(),
                repo: "rho".into(),
                ref_name: Some("main".into()),
                path: "crates/rho-engine".into(),
            })
        );
    }

    #[test]
    fn parse_repo_url() {
        let url = "https://github.com/casonadams/rho.git";
        assert_eq!(
            parse_github_url(url),
            Some(GitHubUrl::Repo {
                owner: "casonadams".into(),
                repo: "rho".into(),
            })
        );
    }

    #[test]
    fn parse_non_github_urls() {
        assert_eq!(parse_github_url("https://gitlab.com/owner/repo/issues/1"), None);
        assert_eq!(parse_github_url("https://example.com"), None);
        assert_eq!(parse_github_url("not-a-url"), None);
    }

    #[test]
    fn format_issue_renders_metadata_and_comments() {
        let issue_json = serde_json::json!({
            "title": "Add specialized fetch extractors",
            "number": 38,
            "state": "open",
            "user": { "login": "ryaminal" },
            "created_at": "2026-09-21T23:32:08Z",
            "updated_at": "2026-09-22T10:00:00Z",
            "body": "When fetching URLs from github.com, standard scraping fails.",
            "labels": [{ "name": "enhancement" }, { "name": "web" }],
            "comments": 1
        });
        let comments_json = vec![serde_json::json!({
            "user": { "login": "casonadams" },
            "created_at": "2026-09-22T12:00:00Z",
            "body": "Sounds great, will implement!"
        })];

        let md = format_issue(&issue_json, &comments_json);
        assert!(md.contains("# Add specialized fetch extractors"));
        assert!(md.contains("**#38** · open · opened by @ryaminal"));
        assert!(md.contains("Created: 2026-09-21T23:32:08Z · Updated: 2026-09-22T10:00:00Z"));
        assert!(md.contains("Labels: enhancement, web"));
        assert!(md.contains("When fetching URLs from github.com, standard scraping fails."));
        assert!(md.contains("## Comments (1)"));
        assert!(md.contains("### @casonadams · 2026-09-22T12:00:00Z"));
        assert!(md.contains("Sounds great, will implement!"));
    }

    #[test]
    fn format_pull_request_renders_branches_diff_and_merged_state() {
        let pr_json = serde_json::json!({
            "title": "feat: add specialized extractors",
            "number": 42,
            "state": "closed",
            "merged_at": "2026-09-25T14:00:00Z",
            "user": { "login": "casonadams" },
            "created_at": "2026-09-25T10:00:00Z",
            "base": { "ref": "main" },
            "head": { "ref": "feature/38-fetch-extractors" },
            "additions": 150,
            "deletions": 10,
            "changed_files": 4,
            "body": "Implements GitHub and YouTube extractors."
        });
        let diff = "+fn extract_github() {}\n-fn old() {}\n";

        let md = format_pull_request(&pr_json, &[], Some(diff));
        assert!(md.contains("# feat: add specialized extractors"));
        assert!(md.contains("**#42** · merged · opened by @casonadams"));
        assert!(md.contains("Branches: `main` ← `feature/38-fetch-extractors`"));
        assert!(md.contains("4 files changed · +150 −10"));
        assert!(md.contains("```diff\n+fn extract_github() {}\n-fn old() {}\n```"));
    }

    #[test]
    fn format_commit_renders_stats_and_patches() {
        let commit_json = serde_json::json!({
            "sha": "abcdef1234567890abcdef1234567890abcdef12",
            "commit": {
                "message": "feat(web): add extractor\n\nDetailed commit body here.",
                "author": { "name": "Cason Adams", "date": "2026-09-25T15:00:00Z" }
            },
            "author": { "login": "casonadams" },
            "parents": [{ "sha": "1234567890abcdef1234567890abcdef12345678" }],
            "stats": { "additions": 45, "deletions": 2 },
            "files": [
                {
                    "filename": "crates/rho-engine/src/lib.rs",
                    "status": "modified",
                    "additions": 45,
                    "deletions": 2,
                    "patch": "@@ -1,2 +1,3 @@\n+pub mod extractor;\n"
                }
            ]
        });

        let md = format_commit(&commit_json);
        assert!(md.contains("# feat(web): add extractor"));
        assert!(md.contains("**abcdef123456** · authored by @casonadams · 2026-09-25T15:00:00Z"));
        assert!(md.contains("1 files changed · +45 −2"));
        assert!(md.contains("Parents: 1234567890ab"));
        assert!(md.contains("Detailed commit body here."));
        assert!(md.contains("### crates/rho-engine/src/lib.rs"));
        assert!(md.contains("```diff\n@@ -1,2 +1,3 @@\n+pub mod extractor;\n```"));
    }

    #[test]
    fn format_tree_renders_table() {
        let entries = vec![
            serde_json::json!({ "name": "src", "type": "dir" }),
            serde_json::json!({ "name": "Cargo.toml", "type": "file", "size": 1024 }),
        ];

        let md = format_tree(&entries, "casonadams", "rho", Some("main"), "crates");
        assert!(md.contains("# casonadams/rho/crates"));
        assert!(md.contains("**Branch/Ref:** `main`"));
        assert!(md.contains("| 📁 dir | `src` | - |"));
        assert!(md.contains("| 📄 file | `Cargo.toml` | 1024 B |"));
    }

    #[test]
    fn format_repo_renders_summary() {
        let repo_json = serde_json::json!({
            "full_name": "casonadams/rho",
            "description": "Agentic coding harness in Rust",
            "default_branch": "main",
            "stargazers_count": 128,
            "forks_count": 16,
            "open_issues_count": 5
        });

        let md = format_repo(&repo_json);
        assert!(md.contains("# casonadams/rho"));
        assert!(md.contains("Agentic coding harness in Rust"));
        assert!(md.contains("- **Default branch:** `main`"));
        assert!(md.contains("- **Stars:** 128"));
        assert!(md.contains("- **Forks:** 16"));
        assert!(md.contains("- **Open issues:** 5"));
    }

    #[test]
    fn test_check_github_status() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert!(check_github_status(reqwest::StatusCode::OK, &headers, "/test").is_ok());

        assert!(check_github_status(reqwest::StatusCode::NOT_FOUND, &headers, "/not-found").is_err());

        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        let err = check_github_status(reqwest::StatusCode::FORBIDDEN, &headers, "/rate-limited").unwrap_err();
        assert!(err.to_string().contains("rate limit exceeded"));
    }

    #[tokio::test]
    async fn test_fetch_github_api_url_mock() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        crate::install_crypto_provider();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = r#"{"name":"rho"}"#;

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let res = fetch_github_api_url(&client, &format!("http://{addr}"), 5, None).await;
        assert_eq!(res.unwrap(), body);
    }
}
