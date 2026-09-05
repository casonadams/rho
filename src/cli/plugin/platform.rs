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
    let mut best_match: Option<(&'a str, i32)> = None;

    for asset in assets {
        if is_non_binary_asset(asset) {
            continue;
        }

        if let Some(score) = score_asset(asset, platform) {
            match best_match {
                Some((_, best_score)) if score > best_score => {
                    best_match = Some((asset.as_str(), score));
                }
                None => {
                    best_match = Some((asset.as_str(), score));
                }
                _ => {}
            }
        }
    }

    best_match
        .map(|(name, _)| name)
        .ok_or_else(|| PlatformMatchError::NoMatchingAsset {
            target_triple: platform.target_triple().to_string(),
            available: assets.to_vec(),
        })
}

fn is_non_binary_asset(name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        ".sha256",
        ".sha512",
        ".sha1",
        ".md5",
        ".sig",
        ".asc",
        ".txt",
        "checksum",
        "sha256sum",
        "source",
    ];
    let lower = name.to_ascii_lowercase();
    EXCLUDED.iter().any(|&pat| lower.contains(pat))
}

fn score_asset(name: &str, platform: &Platform) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    if lower.contains(platform.target_triple()) {
        return Some(100);
    }
    if !matches_os(&lower, platform.os) || !matches_arch(&lower, platform.arch) {
        return None;
    }
    let mut score = 50;
    if lower.ends_with(".tar.gz") || lower.ends_with(".zip") || lower.ends_with(".tar.xz") {
        score += 20;
    }
    if platform.os == Os::Linux && lower.contains("gnu") {
        score += 5;
    }
    Some(score)
}

fn matches_os(lower: &str, os: Os) -> bool {
    match os {
        Os::Macos => {
            (lower.contains("apple-darwin") || lower.contains("darwin") || lower.contains("macos"))
                && !lower.contains("linux")
                && !lower.contains("windows")
        }
        Os::Linux => {
            (lower.contains("unknown-linux") || lower.contains("linux"))
                && !lower.contains("darwin")
                && !lower.contains("macos")
                && !lower.contains("windows")
        }
        Os::Windows => {
            (lower.contains("windows") || lower.contains("win64") || lower.ends_with(".exe"))
                && !lower.contains("darwin")
                && !lower.contains("linux")
        }
    }
}

fn matches_arch(lower: &str, arch: Arch) -> bool {
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
mod tests;
