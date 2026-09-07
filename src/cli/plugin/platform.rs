#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "macos" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    pub fn current() -> Option<Self> {
        match std::env::consts::ARCH {
            "x86_64" => Some(Self::X86_64),
            "aarch64" => Some(Self::Aarch64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Option<Self> {
        Some(Self {
            os: Os::current()?,
            arch: Arch::current()?,
        })
    }

    pub fn target_triple(&self) -> &'static str {
        match (self.arch, self.os) {
            (Arch::Aarch64, Os::Macos) => "aarch64-apple-darwin",
            (Arch::X86_64, Os::Macos) => "x86_64-apple-darwin",
            (Arch::X86_64, Os::Linux) => "x86_64-unknown-linux-gnu",
            (Arch::Aarch64, Os::Linux) => "aarch64-unknown-linux-gnu",
            (Arch::X86_64, Os::Windows) => "x86_64-pc-windows-msvc",
            (Arch::Aarch64, Os::Windows) => "aarch64-pc-windows-msvc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformMatchError {
    #[error("no compatible release asset found for platform '{target_triple}'")]
    NoMatchingAsset {
        target_triple: String,
        available: Vec<String>,
    },
}

pub fn match_platform_asset<'a>(platform: &Platform, assets: &'a [String]) -> Result<&'a str, PlatformMatchError> {
    assets
        .iter()
        .filter(|a| !is_non_binary_asset(a))
        .filter_map(|a| score_asset(a, platform).map(|s| (a.as_str(), s)))
        .max_by_key(|(_, score)| *score)
        .map(|(name, _)| name)
        .ok_or_else(|| PlatformMatchError::NoMatchingAsset {
            target_triple: platform.target_triple().to_string(),
            available: assets.to_vec(),
        })
}

fn is_non_binary_asset(name: &str) -> bool {
    const EXCLUDED_EXTENSIONS: &[&str] = &[
        ".sha256", ".sha512", ".sha1", ".md5", ".sig", ".asc", ".txt", ".deb", ".rpm", ".pkg", ".dmg", ".msi",
    ];
    let lower = name.to_ascii_lowercase();
    if EXCLUDED_EXTENSIONS.iter().any(|&ext| lower.ends_with(ext)) {
        return true;
    }
    lower == "checksums"
        || lower.starts_with("checksums.")
        || lower.starts_with("sha256sum")
        || lower.starts_with("sha512sum")
        || lower == "source.tar.gz"
        || lower == "source.zip"
}

fn base_asset_score(lower: &str, platform: &Platform) -> Option<i32> {
    if lower.contains(platform.target_triple()) {
        return Some(100);
    }
    if !matches_os(lower, platform.os) || !matches_arch(lower, platform.os, platform.arch) {
        return None;
    }
    if platform.os == Os::Macos && (lower.contains("universal") || lower.contains("all")) {
        Some(40)
    } else {
        Some(50)
    }
}

fn score_asset(name: &str, platform: &Platform) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    let mut score = base_asset_score(&lower, platform)?;
    if lower.ends_with(".tar.gz") || lower.ends_with(".zip") || lower.ends_with(".tar.xz") || lower.ends_with(".exe") {
        score += 20;
    }
    if platform.os == Os::Linux && lower.contains("gnu") {
        score += 5;
    }
    Some(score)
}

fn matches_os(lower: &str, os: Os) -> bool {
    let has_darwin = lower.contains("apple-darwin") || lower.contains("darwin") || lower.contains("macos");
    let has_linux = lower.contains("unknown-linux") || lower.contains("linux");
    let has_windows = lower.contains("windows") || lower.contains("win64") || lower.ends_with(".exe");
    match os {
        Os::Macos => has_darwin && !has_linux && !has_windows,
        Os::Linux => has_linux && !has_darwin && !has_windows,
        Os::Windows => has_windows && !has_darwin && !has_linux,
    }
}

fn matches_arch(lower: &str, os: Os, arch: Arch) -> bool {
    if os == Os::Macos && (lower.contains("universal") || lower.contains("all")) {
        return true;
    }
    match arch {
        Arch::Aarch64 => (lower.contains("aarch64") || lower.contains("arm64")) && !lower.contains("x86_64"),
        Arch::X86_64 => {
            (lower.contains("x86_64") || lower.contains("amd64") || lower.contains("x64"))
                && !lower.contains("aarch64")
                && !lower.contains("arm64")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_triples() {
        let cases = [
            (Os::Macos, Arch::Aarch64, "aarch64-apple-darwin"),
            (Os::Macos, Arch::X86_64, "x86_64-apple-darwin"),
            (Os::Linux, Arch::X86_64, "x86_64-unknown-linux-gnu"),
            (Os::Linux, Arch::Aarch64, "aarch64-unknown-linux-gnu"),
            (Os::Windows, Arch::X86_64, "x86_64-pc-windows-msvc"),
            (Os::Windows, Arch::Aarch64, "aarch64-pc-windows-msvc"),
        ];
        for (os, arch, expected) in cases {
            assert_eq!(Platform { os, arch }.target_triple(), expected);
        }
    }

    #[test]
    fn test_match_exact_triple_asset() {
        let platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let assets = vec![
            "rho-plugin-permission-x86_64-apple-darwin.tar.gz".to_string(),
            "rho-plugin-permission-aarch64-apple-darwin.tar.gz".to_string(),
            "rho-plugin-permission-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-plugin-permission-aarch64-apple-darwin.tar.gz");
    }

    #[test]
    fn test_match_linux_asset() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        let assets = vec![
            "plugin-linux-amd64.tar.gz".to_string(),
            "plugin-darwin-arm64.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "plugin-linux-amd64.tar.gz");
    }

    #[test]
    fn test_match_windows_asset() {
        let platform = Platform {
            os: Os::Windows,
            arch: Arch::X86_64,
        };
        let assets = vec![
            "plugin-x86_64-pc-windows-msvc.zip".to_string(),
            "plugin-x86_64-apple-darwin.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "plugin-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn test_ignores_checksums_and_signatures() {
        let platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let assets = vec![
            "rho-plugin-permission-aarch64-apple-darwin.tar.gz.sha256".to_string(),
            "checksums.txt".to_string(),
            "rho-plugin-permission-aarch64-apple-darwin.tar.gz.sig".to_string(),
            "rho-plugin-permission-aarch64-apple-darwin.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-plugin-permission-aarch64-apple-darwin.tar.gz");
    }

    #[test]
    fn test_no_matching_asset_returns_error() {
        let platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let assets = vec![
            "rho-plugin-permission-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "rho-plugin-permission-x86_64-pc-windows-msvc.zip".to_string(),
        ];
        let err = match_platform_asset(&platform, &assets).unwrap_err();
        assert!(matches!(err, PlatformMatchError::NoMatchingAsset { .. }));
    }

    #[test]
    fn test_plugin_named_checksum_or_source_is_not_falsely_excluded() {
        let platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let assets = vec![
            "rho-plugin-checksum-aarch64-apple-darwin.tar.gz".to_string(),
            "checksums.txt".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-plugin-checksum-aarch64-apple-darwin.tar.gz");

        let assets_src = vec![
            "rho-plugin-source-aarch64-apple-darwin.tar.gz".to_string(),
            "source.tar.gz".to_string(),
        ];
        let matched_src = match_platform_asset(&platform, &assets_src).unwrap();
        assert_eq!(matched_src, "rho-plugin-source-aarch64-apple-darwin.tar.gz");
    }

    #[test]
    fn test_ignores_installer_packages_over_archives() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        let assets = vec![
            "rho-plugin-git-x86_64-unknown-linux-gnu.deb".to_string(),
            "rho-plugin-git-x86_64-unknown-linux-gnu.rpm".to_string(),
            "rho-plugin-git-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-plugin-git-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn test_matches_windows_exe_binary() {
        let platform = Platform {
            os: Os::Windows,
            arch: Arch::X86_64,
        };
        let assets = vec![
            "rho-plugin-git-x86_64-pc-windows-msvc.exe".to_string(),
            "rho-plugin-git-x86_64-apple-darwin.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-plugin-git-x86_64-pc-windows-msvc.exe");
    }

    #[test]
    fn test_matches_macos_universal_binary_when_exact_unavailable() {
        let platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let assets = vec![
            "plugin-universal-apple-darwin.tar.gz".to_string(),
            "plugin-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "plugin-universal-apple-darwin.tar.gz");

        let platform_x86 = Platform {
            os: Os::Macos,
            arch: Arch::X86_64,
        };
        let matched_x86 = match_platform_asset(&platform_x86, &assets).unwrap();
        assert_eq!(matched_x86, "plugin-universal-apple-darwin.tar.gz");

        let assets_with_exact = vec![
            "plugin-universal-apple-darwin.tar.gz".to_string(),
            "plugin-aarch64-apple-darwin.tar.gz".to_string(),
        ];
        let matched_exact = match_platform_asset(&platform, &assets_with_exact).unwrap();
        assert_eq!(matched_exact, "plugin-aarch64-apple-darwin.tar.gz");
    }
}
