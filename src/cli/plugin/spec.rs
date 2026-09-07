//! Plugin specification parser, version comparator, and collision validator.

use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const DEFAULT_GITHUB_ORG: &str = "casonadams";

// ---------------------------------------------------------------------------
// Plugin specification parsing
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Simple version comparison
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Deduplication & collision validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DuplicatePluginError {
    #[error("plugin '{0}' is already configured (use --force or --replace to overwrite)")]
    Name(String),

    #[error("plugin command '{command}' collides with existing plugin '{existing_plugin}'")]
    Command { existing_plugin: String, command: String },

    #[error("plugin executable path '{path}' collides with existing plugin '{existing_plugin}'")]
    Path { existing_plugin: String, path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCandidate {
    pub name: String,
    pub command: String,
    pub path: PathBuf,
    pub force: bool,
}

fn cmd_err(existing_plugin: &str, command: &str) -> DuplicatePluginError {
    DuplicatePluginError::Command {
        existing_plugin: existing_plugin.to_string(),
        command: command.to_string(),
    }
}

fn check_name_conflict(existing_name: &str, candidate: &PluginCandidate) -> Result<bool, DuplicatePluginError> {
    let same = existing_name == candidate.name || strip_prefix(existing_name) == strip_prefix(&candidate.name);
    if same {
        if !candidate.force {
            return Err(DuplicatePluginError::Name(existing_name.to_string()));
        }
        return Ok(true);
    }
    Ok(false)
}

fn check_path_conflict(
    existing_name: &str,
    existing_path: &Path,
    candidate_path: &Path,
) -> Result<(), DuplicatePluginError> {
    if !candidate_path.as_os_str().is_empty()
        && !existing_path.as_os_str().is_empty()
        && existing_path == candidate_path
    {
        return Err(DuplicatePluginError::Path {
            existing_plugin: existing_name.to_string(),
            path: candidate_path.to_path_buf(),
        });
    }
    Ok(())
}

fn check_command_overlap(
    existing_name: &str,
    existing_cfg: &PluginConfig,
    candidate: &PluginCandidate,
) -> Result<(), DuplicatePluginError> {
    if existing_cfg.command.as_deref() == Some(&candidate.command) {
        return Err(cmd_err(existing_name, &candidate.command));
    }
    let cand_p = (!candidate.path.as_os_str().is_empty()).then_some(candidate.path.as_path());
    let exist_p = (!existing_cfg.path.as_os_str().is_empty()).then_some(existing_cfg.path.as_path());
    let cand_matches = cand_p.is_some_and(|p| {
        existing_cfg.command.as_deref() == p.to_str()
            || existing_cfg.command.as_deref() == p.file_name().and_then(|f| f.to_str())
    });
    let exist_matches = exist_p.is_some_and(|p| {
        p.to_str() == Some(&candidate.command) || p.file_name().and_then(|f| f.to_str()) == Some(&candidate.command)
    });
    if cand_matches || exist_matches {
        return Err(cmd_err(existing_name, &candidate.command));
    }
    Ok(())
}

pub fn validate_no_duplicates(
    existing_plugins: &BTreeMap<String, PluginConfig>,
    candidate: &PluginCandidate,
) -> Result<(), DuplicatePluginError> {
    for (existing_name, existing_cfg) in existing_plugins {
        if check_name_conflict(existing_name, candidate)? {
            continue;
        }
        check_path_conflict(existing_name, &existing_cfg.path, &candidate.path)?;
        check_command_overlap(existing_name, existing_cfg, candidate)?;
    }
    Ok(())
}

fn strip_prefix(name: &str) -> &str {
    name.strip_prefix("rho-plugin-").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acceptance_criteria_examples() {
        let cases = [
            ("foo", ("rho-plugin-foo", DEFAULT_GITHUB_ORG, "rho-plugin-foo", None)),
            (
                "foo@1.0.0",
                ("rho-plugin-foo", DEFAULT_GITHUB_ORG, "rho-plugin-foo", Some("1.0.0")),
            ),
            ("org/repo", ("repo", "org", "repo", None)),
            ("org/repo@v1.0.0", ("repo", "org", "repo", Some("v1.0.0"))),
            ("https://github.com/org/repo", ("repo", "org", "repo", None)),
        ];
        for (input, (name, owner, repo, tag)) in cases {
            let spec = PluginSpec::parse(input).unwrap();
            assert_eq!(
                (
                    spec.name.as_str(),
                    spec.owner.as_str(),
                    spec.repo.as_str(),
                    spec.tag.as_deref()
                ),
                (name, owner, repo, tag)
            );
        }
    }

    #[test]
    fn test_parse_bare_short_name() {
        let spec = PluginSpec::parse("permission").unwrap();
        let actual = (
            spec.name.as_str(),
            spec.owner.as_str(),
            spec.repo.as_str(),
            spec.executable_name.as_str(),
            spec.tag.as_deref(),
        );
        assert_eq!(
            actual,
            (
                "rho-plugin-permission",
                DEFAULT_GITHUB_ORG,
                "rho-plugin-permission",
                "rho-plugin-permission",
                None
            )
        );
        assert_eq!(
            (spec.github_repo().as_str(), spec.short_name()),
            ("casonadams/rho-plugin-permission", "permission")
        );
    }

    #[test]
    fn test_parse_bare_prefixed_name() {
        let spec = PluginSpec::parse("rho-plugin-git").unwrap();
        let actual = (
            spec.name.as_str(),
            spec.owner.as_str(),
            spec.repo.as_str(),
            spec.executable_name.as_str(),
            spec.tag.as_deref(),
            spec.short_name(),
        );
        assert_eq!(
            actual,
            (
                "rho-plugin-git",
                DEFAULT_GITHUB_ORG,
                "rho-plugin-git",
                "rho-plugin-git",
                None,
                "git"
            )
        );
    }

    #[test]
    fn test_parse_pinned_versions() {
        let spec = PluginSpec::parse("permission@0.3.0").unwrap();
        assert_eq!(spec.name, "rho-plugin-permission");
        assert_eq!(spec.tag, Some("0.3.0".to_string()));

        let spec = PluginSpec::parse("rho-plugin-shell@v1.2.3").unwrap();
        assert_eq!(spec.name, "rho-plugin-shell");
        assert_eq!(spec.tag, Some("v1.2.3".to_string()));
    }

    #[test]
    fn test_parse_github_slug() {
        let cases = [
            (
                "casonadams/rho-plugin-permission",
                ("rho-plugin-permission", "casonadams", "rho-plugin-permission", None),
            ),
            (
                "custom-org/custom-plugin@2.0.0",
                ("custom-plugin", "custom-org", "custom-plugin", Some("2.0.0")),
            ),
            (
                "custom-org/custom-plugin/",
                ("custom-plugin", "custom-org", "custom-plugin", None),
            ),
        ];
        for (input, (name, owner, repo, tag)) in cases {
            let spec = PluginSpec::parse(input).unwrap();
            let actual = (
                spec.name.as_str(),
                spec.owner.as_str(),
                spec.repo.as_str(),
                spec.tag.as_deref(),
            );
            assert_eq!(actual, (name, owner, repo, tag));
        }
    }

    #[test]
    fn test_parse_github_url_standard() {
        for url in [
            "https://github.com/casonadams/rho-plugin-permission",
            "https://github.com/casonadams/rho-plugin-permission.git",
            "https://github.com/casonadams/rho-plugin-permission/",
        ] {
            let spec = PluginSpec::parse(url).unwrap();
            assert_eq!(
                (spec.name.as_str(), spec.owner.as_str(), spec.repo.as_str()),
                ("rho-plugin-permission", "casonadams", "rho-plugin-permission")
            );
        }
    }

    #[test]
    fn test_parse_github_url_release_and_tree() {
        let cases = [
            ("https://github.com/org/repo@v1.0.0", "v1.0.0"),
            ("https://github.com/org/repo/releases/tag/v1.2.3", "v1.2.3"),
            ("https://github.com/org/repo/tree/v2.0.0", "v2.0.0"),
        ];
        for (url, tag) in cases {
            let spec = PluginSpec::parse(url).unwrap();
            assert_eq!(
                (spec.name.as_str(), spec.owner.as_str(), spec.tag.as_deref()),
                ("repo", "org", Some(tag))
            );
        }
    }

    #[test]
    fn test_parse_invalid_empty_and_tag() {
        for input in ["", "   ", "@1.0.0"] {
            assert_eq!(PluginSpec::parse(input), Err(PluginSpecError::Empty));
        }
        assert_eq!(PluginSpec::parse("plugin@"), Err(PluginSpecError::EmptyTag));
    }

    #[test]
    fn test_parse_invalid_urls_and_names() {
        for host in [
            "http://github.com/org/repo",
            "https://gitlab.com/org/repo",
            "git@github.com:org/repo.git",
            "ftp://github.com/org/repo",
        ] {
            assert!(PluginSpec::parse(host).is_err());
        }
        for invalid in [
            "org/repo/extra",
            "invalid name with spaces",
            "https://github.com/org/repo/extra/path",
            "rho-plugin-",
        ] {
            assert!(PluginSpec::parse(invalid).is_err());
        }
    }

    #[test]
    fn test_from_str_and_display() {
        let spec: PluginSpec = "permission@0.1.0".parse().unwrap();
        assert_eq!(spec.name, "rho-plugin-permission");
        assert_eq!(spec.tag.as_deref(), Some("0.1.0"));
        assert_eq!(format!("{spec}"), "casonadams/rho-plugin-permission@0.1.0");

        let spec_no_tag: PluginSpec = "org/my-plugin".parse().unwrap();
        assert_eq!(format!("{spec_no_tag}"), "org/my-plugin");
    }

    #[test]
    fn test_parse_simple_version() {
        let cases = [
            ("1.2.3", (1, 2, 3, None)),
            ("v0.3.0", (0, 3, 0, None)),
            ("v1.0.0-rc.1", (1, 0, 0, Some("rc.1".to_string()))),
        ];
        for (input, (maj, min, pat, pre)) in cases {
            let v = SimpleVersion::parse(input).unwrap();
            assert_eq!((v.major, v.minor, v.patch, v.prerelease), (maj, min, pat, pre));
        }
    }

    #[test]
    fn test_is_newer_than() {
        let cases = [
            ("0.3.1", "0.3.0", true),
            ("0.4.0", "0.3.1", true),
            ("1.0.0", "0.4.0", true),
            ("1.0.0", "1.0.0-rc.1", true),
            ("0.3.0", "0.3.1", false),
            ("0.3.0", "0.3.0", false),
            ("1.0.0-rc.1", "1.0.0", false),
        ];
        for (v_new, v_old, expected) in cases {
            let n = SimpleVersion::parse(v_new).unwrap();
            let o = SimpleVersion::parse(v_old).unwrap();
            assert_eq!(n.is_newer_than(&o), expected);
        }
    }

    #[test]
    fn test_is_update_available() {
        let cases = [
            ("0.3.0", "v0.3.1", true),
            ("v0.3.0", "0.4.0", true),
            ("1.0.0-rc.1", "1.0.0", true),
            ("0.3.1", "v0.3.1", false),
            ("v0.3.1", "0.3.1", false),
            ("0.4.0", "0.3.1", false),
            ("1.0.0", "1.0.0", false),
        ];
        for (current, latest, expected) in cases {
            assert_eq!(is_update_available(current, latest), expected);
        }
    }

    #[test]
    fn test_is_update_available_unparseable_fallback() {
        assert!(is_update_available("custom-1", "custom-2"));
        assert!(!is_update_available("same", "same"));
    }

    fn make_plugin(command: Option<&str>, path: &str) -> PluginConfig {
        PluginConfig {
            command: command.map(ToString::to_string),
            path: PathBuf::from(path),
            ..Default::default()
        }
    }

    #[test]
    fn test_dedup_empty_plugins() {
        let plugins = BTreeMap::new();
        let candidate = PluginCandidate {
            name: "rho-plugin-permission".to_string(),
            command: "rho-plugin-permission".to_string(),
            path: PathBuf::from("/bin/rho-plugin-permission"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert!(res.is_ok());
    }

    #[test]
    fn test_dedup_distinct_plugin() {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "rho-plugin-git".to_string(),
            make_plugin(Some("rho-plugin-git"), "/bin/rho-plugin-git"),
        );

        let candidate = PluginCandidate {
            name: "rho-plugin-shell".to_string(),
            command: "rho-plugin-shell".to_string(),
            path: PathBuf::from("/bin/rho-plugin-shell"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert!(res.is_ok());
    }

    #[test]
    fn test_dedup_exact_name_collision() {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "rho-plugin-git".to_string(),
            make_plugin(Some("rho-plugin-git"), "/bin/rho-plugin-git"),
        );

        let candidate = PluginCandidate {
            name: "rho-plugin-git".to_string(),
            command: "rho-plugin-git".to_string(),
            path: PathBuf::from("/bin/rho-plugin-git"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert_eq!(res, Err(DuplicatePluginError::Name("rho-plugin-git".to_string())));

        let candidate_force = PluginCandidate {
            name: "rho-plugin-git".to_string(),
            command: "rho-plugin-git".to_string(),
            path: PathBuf::from("/bin/rho-plugin-git"),
            force: true,
        };
        let res_force = validate_no_duplicates(&plugins, &candidate_force);
        assert!(res_force.is_ok());
    }

    #[test]
    fn test_dedup_prefix_normalized_name_collision() {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "permission".to_string(),
            make_plugin(Some("rho-plugin-permission"), "/bin/rho-plugin-permission"),
        );

        let candidate = PluginCandidate {
            name: "rho-plugin-permission".to_string(),
            command: "rho-plugin-permission".to_string(),
            path: PathBuf::from("/bin/rho-plugin-permission"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert_eq!(res, Err(DuplicatePluginError::Name("permission".to_string())));

        let candidate_force = PluginCandidate {
            name: "rho-plugin-permission".to_string(),
            command: "rho-plugin-permission".to_string(),
            path: PathBuf::from("/bin/rho-plugin-permission"),
            force: true,
        };
        let res_force = validate_no_duplicates(&plugins, &candidate_force);
        assert!(res_force.is_ok());
    }

    #[test]
    fn test_dedup_command_collision_with_other_plugin() {
        let plugins = BTreeMap::from([(
            "custom-git".to_string(),
            make_plugin(Some("git-helper"), "/bin/git-helper"),
        )]);
        let candidate = PluginCandidate {
            name: "other-git".to_string(),
            command: "git-helper".to_string(),
            path: PathBuf::from("/bin/other-helper"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert_eq!(
            res,
            Err(DuplicatePluginError::Command {
                existing_plugin: "custom-git".to_string(),
                command: "git-helper".to_string()
            })
        );

        let candidate_force = PluginCandidate {
            name: "other-git".to_string(),
            command: "git-helper".to_string(),
            path: PathBuf::from("/bin/other-helper"),
            force: true,
        };
        assert!(validate_no_duplicates(&plugins, &candidate_force).is_err());
    }

    #[test]
    fn test_dedup_path_collision_with_other_plugin() {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "plugin-a".to_string(),
            make_plugin(Some("cmd-a"), "/usr/local/bin/shared-tool"),
        );

        let candidate = PluginCandidate {
            name: "plugin-b".to_string(),
            command: "cmd-b".to_string(),
            path: PathBuf::from("/usr/local/bin/shared-tool"),
            force: false,
        };
        let res = validate_no_duplicates(&plugins, &candidate);
        assert_eq!(
            res,
            Err(DuplicatePluginError::Path {
                existing_plugin: "plugin-a".to_string(),
                path: PathBuf::from("/usr/local/bin/shared-tool"),
            })
        );
    }

    #[test]
    fn test_dedup_cmd_matches_filename() {
        let plugins = BTreeMap::from([("tool-one".to_string(), make_plugin(None, "/usr/local/bin/my-tool"))]);
        let candidate = PluginCandidate {
            name: "tool-two".to_string(),
            command: "my-tool".to_string(),
            path: PathBuf::new(),
            force: false,
        };
        assert!(matches!(
            validate_no_duplicates(&plugins, &candidate),
            Err(DuplicatePluginError::Command { .. })
        ));
    }

    #[test]
    fn test_dedup_path_matches_cmd_or_exact() {
        let plugins2 = BTreeMap::from([("tool-three".to_string(), make_plugin(Some("runner"), ""))]);
        let candidate2 = PluginCandidate {
            name: "tool-four".to_string(),
            command: "other".to_string(),
            path: PathBuf::from("/opt/bin/runner"),
            force: false,
        };
        assert!(matches!(
            validate_no_duplicates(&plugins2, &candidate2),
            Err(DuplicatePluginError::Command { .. })
        ));

        let plugins3 = BTreeMap::from([("tool-five".to_string(), make_plugin(Some("/exact/path/runner"), ""))]);
        let candidate3 = PluginCandidate {
            name: "tool-six".to_string(),
            command: "other".to_string(),
            path: PathBuf::from("/exact/path/runner"),
            force: false,
        };
        assert!(matches!(
            validate_no_duplicates(&plugins3, &candidate3),
            Err(DuplicatePluginError::Command { .. })
        ));
    }
}
