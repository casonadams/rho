use std::str::FromStr;

pub const DEFAULT_GITHUB_ORG: &str = "casonadams";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginSpecError {
    #[error("plugin specifier cannot be empty")]
    Empty,
    #[error("unsupported protocol in '{0}': only HTTPS is supported")]
    InsecureHttp(String),
    #[error("unsupported host in '{0}': only github.com is supported")]
    UnsupportedHost(String),
    #[error("invalid repository URL '{0}': expected https://github.com/owner/repo")]
    InvalidUrl(String),
    #[error("invalid GitHub repository slug '{0}': expected 'owner/repo'")]
    InvalidSlug(String),
    #[error("empty version tag specified")]
    EmptyTag,
    #[error("invalid plugin name '{0}'")]
    InvalidName(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    pub name: String,
    pub owner: String,
    pub repo: String,
    pub tag: Option<String>,
    pub executable_name: String,
}

fn split_raw_target(trimmed: &str) -> Result<(&str, Option<String>), PluginSpecError> {
    if trimmed.is_empty() {
        return Err(PluginSpecError::Empty);
    }
    if trimmed.starts_with("git@") {
        return Err(PluginSpecError::UnsupportedHost(trimmed.to_string()));
    }
    match trimmed.split_once('@') {
        Some((b, t)) => {
            let tag_trimmed = t.trim();
            if tag_trimmed.is_empty() {
                return Err(PluginSpecError::EmptyTag);
            }
            if b.trim().is_empty() {
                return Err(PluginSpecError::Empty);
            }
            Ok((b.trim(), Some(tag_trimmed.to_string())))
        }
        None => Ok((trimmed, None)),
    }
}

fn resolve_repo_coords(raw_target: &str) -> Result<(String, String, Option<String>), PluginSpecError> {
    if raw_target.starts_with("http://") {
        return Err(PluginSpecError::InsecureHttp(raw_target.to_string()));
    }
    if let Some(url_str) = raw_target.strip_prefix("https://") {
        return parse_github_url(url_str, raw_target);
    }
    if raw_target.starts_with("github.com/") {
        return parse_github_url(raw_target, raw_target);
    }
    if raw_target.contains("://") {
        return Err(PluginSpecError::UnsupportedHost(raw_target.to_string()));
    }
    let (o, r) = if raw_target.contains('/') {
        parse_github_slug(raw_target)?
    } else {
        parse_bare_name(raw_target)?
    };
    Ok((o, r, None))
}

impl PluginSpec {
    pub fn parse(input: &str) -> Result<Self, PluginSpecError> {
        let (raw_target, tag) = split_raw_target(input.trim())?;
        let (owner, repo, url_tag) = resolve_repo_coords(raw_target)?;
        let tag = tag.or(url_tag);
        let executable_name = repo.clone();

        Ok(Self {
            name: repo.clone(),
            owner,
            repo: executable_name.clone(),
            tag,
            executable_name,
        })
    }

    pub fn github_repo(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    pub fn short_name(&self) -> &str {
        self.name.strip_prefix("rho-plugin-").unwrap_or(&self.name)
    }
}

impl FromStr for PluginSpec {
    type Err = PluginSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl std::fmt::Display for PluginSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tag {
            Some(tag) => write!(f, "{}/{}@{tag}", self.owner, self.repo),
            None => write!(f, "{}/{}", self.owner, self.repo),
        }
    }
}

fn parse_github_url(url_without_scheme: &str, full: &str) -> Result<(String, String, Option<String>), PluginSpecError> {
    let parts: Vec<&str> = url_without_scheme.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts[0] != "github.com" {
        return Err(PluginSpecError::UnsupportedHost(full.to_string()));
    }
    let tag = match parts.as_slice() {
        [_, _, _, "releases", "tag", t] | [_, _, _, "tree", t] => Some((*t).to_string()),
        [_, _, _] => None,
        _ => return Err(PluginSpecError::InvalidUrl(full.to_string())),
    };
    let owner = parts[1].to_string();
    let repo = parts[2].strip_suffix(".git").unwrap_or(parts[2]).to_string();
    if owner.is_empty() || repo.is_empty() {
        return Err(PluginSpecError::InvalidUrl(full.to_string()));
    }
    Ok((owner, repo, tag))
}

fn parse_github_slug(slug: &str) -> Result<(String, String), PluginSpecError> {
    let trimmed = slug.trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(PluginSpecError::InvalidSlug(slug.to_string()));
    }
    let owner = parts[0].to_string();
    let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]).to_string();
    if repo.is_empty() {
        return Err(PluginSpecError::InvalidSlug(slug.to_string()));
    }
    Ok((owner, repo))
}

fn parse_bare_name(name: &str) -> Result<(String, String), PluginSpecError> {
    if name.is_empty() || name == "rho-plugin-" || name.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
        return Err(PluginSpecError::InvalidName(name.to_string()));
    }
    let repo = if name.starts_with("rho-plugin-") {
        name.to_string()
    } else {
        format!("rho-plugin-{name}")
    };
    Ok((DEFAULT_GITHUB_ORG.to_string(), repo))
}

#[cfg(test)]
mod tests;
