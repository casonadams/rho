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

fn score_asset(name: &str, platform: &Platform) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    let mut score = if lower.contains(platform.target_triple()) {
        100
    } else {
        if !matches_os(&lower, platform.os) || !matches_arch(&lower, platform.os, platform.arch) {
            return None;
        }
        if platform.os == Os::Macos && (lower.contains("universal") || lower.contains("all")) {
            40
        } else {
            50
        }
    };
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
mod tests;
