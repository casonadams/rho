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

impl PluginSpec {
    pub fn parse(input: &str) -> Result<Self, PluginSpecError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(PluginSpecError::Empty);
        }

        let (raw_target, tag) = match trimmed.split_once('@') {
            Some((b, t)) => {
                let tag_trimmed = t.trim();
                if tag_trimmed.is_empty() {
                    return Err(PluginSpecError::EmptyTag);
                }
                (b.trim(), Some(tag_trimmed.to_string()))
            }
            None => (trimmed, None),
        };

        if raw_target.is_empty() {
            return Err(PluginSpecError::Empty);
        }

        let (owner, repo) = if raw_target.starts_with("http://") {
            return Err(PluginSpecError::InsecureHttp(raw_target.to_string()));
        } else if let Some(url_str) = raw_target.strip_prefix("https://") {
            parse_github_url(url_str, raw_target)?
        } else if raw_target.contains('/') {
            parse_github_slug(raw_target)?
        } else {
            parse_bare_name(raw_target)?
        };

        let executable_name = repo.clone();
        let name = repo.clone();

        Ok(Self {
            name,
            owner,
            repo,
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

fn parse_github_url(url_without_scheme: &str, full: &str) -> Result<(String, String), PluginSpecError> {
    let parts: Vec<&str> = url_without_scheme.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts[0] != "github.com" {
        return Err(PluginSpecError::UnsupportedHost(full.to_string()));
    }
    if parts.len() < 3 {
        return Err(PluginSpecError::InvalidUrl(full.to_string()));
    }
    let owner = parts[1].to_string();
    let repo = parts[2].trim_end_matches(".git").to_string();
    if owner.is_empty() || repo.is_empty() {
        return Err(PluginSpecError::InvalidUrl(full.to_string()));
    }
    Ok((owner, repo))
}

fn parse_github_slug(slug: &str) -> Result<(String, String), PluginSpecError> {
    let parts: Vec<&str> = slug.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(PluginSpecError::InvalidSlug(slug.to_string()));
    }
    let owner = parts[0].to_string();
    let repo = parts[1].trim_end_matches(".git").to_string();
    Ok((owner, repo))
}

fn parse_bare_name(name: &str) -> Result<(String, String), PluginSpecError> {
    if name.chars().any(|c| c.is_whitespace() || c == '/' || c == ':') {
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
