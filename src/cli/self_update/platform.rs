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
    let triple = platform.target_triple();

    if let Some(matched) = assets.iter().find(|a| a.contains(triple)) {
        return Ok(matched.as_str());
    }

    let os_hint = match platform.os {
        Os::Macos => "darwin",
        Os::Linux => "linux",
        Os::Windows => "windows",
    };
    let arch_hint = match platform.arch {
        Arch::Aarch64 => "arm64",
        Arch::X86_64 => "x64",
    };

    if let Some(matched) = assets.iter().find(|a| {
        let lower = a.to_ascii_lowercase();
        lower.contains(os_hint) && lower.contains(arch_hint)
    }) {
        return Ok(matched.as_str());
    }

    Err(PlatformMatchError::NoMatchingAsset {
        target_triple: triple.to_string(),
        available: assets.to_vec(),
    })
}
