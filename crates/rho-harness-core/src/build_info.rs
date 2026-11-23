//! Build and runtime identity metadata surfaced in the UI, CLI `--version`,
//! RPC config, and system prompt so both the user and the agent know which
//! exact binary and commit are running.

use chrono::{DateTime, Local};
use std::sync::LazyLock;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("RHO_GIT_SHA");
pub const PROFILE: &str = env!("RHO_BUILD_PROFILE");

struct RuntimeBuildInfo {
    divider_label: String,
    welcome_version: String,
    runtime_summary: String,
}

static BUILD_INFO: LazyLock<RuntimeBuildInfo> = LazyLock::new(|| {
    let exe_path = std::env::current_exe().ok();
    let mtime = exe_path
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Local>::from);
    let short_time = mtime.map(|dt| dt.format("%H:%M").to_string());
    let full_time = mtime.map(|dt| dt.format("%Y-%m-%d %H:%M").to_string());
    let exe_str = exe_path.map(|p| p.display().to_string().replace('\\', "/"));

    RuntimeBuildInfo {
        divider_label: format_divider_label(VERSION, GIT_SHA, short_time.as_deref()),
        welcome_version: format_welcome_version(VERSION, GIT_SHA, short_time.as_deref(), PROFILE),
        runtime_summary: format_runtime_summary(VERSION, GIT_SHA, full_time.as_deref(), PROFILE, exe_str.as_deref()),
    }
});

pub fn format_divider_label(version: &str, git_sha: &str, short_time: Option<&str>) -> String {
    let mut meta = Vec::new();
    if !git_sha.is_empty() {
        meta.push(git_sha);
    }
    if let Some(time) = short_time.filter(|t| !t.is_empty()) {
        meta.push(time);
    }
    if meta.is_empty() {
        format!("rho {version}")
    } else {
        format!("rho {version} ({})", meta.join(" "))
    }
}

pub fn format_welcome_version(version: &str, git_sha: &str, short_time: Option<&str>, profile: &str) -> String {
    let mut meta = Vec::new();
    if !git_sha.is_empty() {
        meta.push(git_sha);
    }
    if let Some(time) = short_time.filter(|t| !t.is_empty()) {
        meta.push(time);
    }
    if !profile.is_empty() {
        meta.push(profile);
    }
    if meta.is_empty() {
        version.to_string()
    } else {
        format!("{version} ({})", meta.join(", "))
    }
}

pub fn format_runtime_summary(
    version: &str,
    git_sha: &str,
    full_time: Option<&str>,
    profile: &str,
    exe_path: Option<&str>,
) -> String {
    let mut meta: Vec<String> = Vec::new();
    if !git_sha.is_empty() {
        meta.push(git_sha.to_string());
    }
    if let Some(time) = full_time.filter(|t| !t.is_empty()) {
        meta.push(format!("built {time}"));
    }
    if !profile.is_empty() {
        meta.push(profile.to_string());
    }
    if let Some(path) = exe_path.filter(|p| !p.is_empty()) {
        meta.push(path.to_string());
    }
    if meta.is_empty() {
        format!("rho {version}")
    } else {
        format!("rho {version} ({})", meta.join(", "))
    }
}

/// Compact label for the top TUI divider, e.g. `rho 0.17.0 (dd39733d0-dirty 15:42)`.
pub fn divider_label() -> &'static str {
    &BUILD_INFO.divider_label
}

/// Detailed version string for the welcome banner and `rho --version`.
pub fn welcome_version() -> &'static str {
    &BUILD_INFO.welcome_version
}

/// Full runtime identity for the system prompt environment footer.
pub fn runtime_summary() -> &'static str {
    &BUILD_INFO.runtime_summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_divider_label_variants() {
        assert_eq!(
            format_divider_label("0.17.0", "abc1234-dirty", Some("15:42")),
            "rho 0.17.0 (abc1234-dirty 15:42)"
        );
        assert_eq!(format_divider_label("0.17.0", "abc1234", None), "rho 0.17.0 (abc1234)");
        assert_eq!(format_divider_label("0.17.0", "", Some("15:42")), "rho 0.17.0 (15:42)");
        assert_eq!(format_divider_label("0.17.0", "", None), "rho 0.17.0");
    }

    #[test]
    fn formats_welcome_version_variants() {
        assert_eq!(
            format_welcome_version("0.17.0", "abc1234-dirty", Some("15:42"), "release"),
            "0.17.0 (abc1234-dirty, 15:42, release)"
        );
        assert_eq!(format_welcome_version("0.17.0", "", None, ""), "0.17.0");
    }

    #[test]
    fn formats_runtime_summary_variants() {
        assert_eq!(
            format_runtime_summary(
                "0.17.0",
                "abc1234-dirty",
                Some("2026-11-23 15:42"),
                "release",
                Some("/home/me/.cargo/bin/rho"),
            ),
            "rho 0.17.0 (abc1234-dirty, built 2026-11-23 15:42, release, /home/me/.cargo/bin/rho)"
        );
        assert_eq!(format_runtime_summary("0.17.0", "", None, "", None), "rho 0.17.0");
    }
}
