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
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Macos => "darwin",
            Self::Linux => "linux",
            Self::Windows => "windows",
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
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Aarch64 => "arm64",
            Self::X86_64 => "x64",
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

fn find_asset_by_hint<'a>(os_hint: &str, arch_hint: &str, assets: &'a [String]) -> Option<&'a str> {
    assets.iter().find_map(|a| {
        let lower = a.to_ascii_lowercase();
        (lower.contains(os_hint) && lower.contains(arch_hint)).then_some(a.as_str())
    })
}

pub fn match_platform_asset<'a>(platform: &Platform, assets: &'a [String]) -> Result<&'a str, PlatformMatchError> {
    let triple = platform.target_triple();

    if let Some(matched) = assets.iter().find(|a| a.contains(triple)) {
        return Ok(matched.as_str());
    }

    if let Some(matched) = find_asset_by_hint(platform.os.hint(), platform.arch.hint(), assets) {
        return Ok(matched);
    }

    Err(PlatformMatchError::NoMatchingAsset {
        target_triple: triple.to_string(),
        available: assets.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_and_arch_hints() {
        assert_eq!(Os::Macos.hint(), "darwin");
        assert_eq!(Os::Linux.hint(), "linux");
        assert_eq!(Os::Windows.hint(), "windows");

        assert_eq!(Arch::Aarch64.hint(), "arm64");
        assert_eq!(Arch::X86_64.hint(), "x64");
    }

    #[test]
    fn test_os_and_arch_current() {
        assert!(Os::current().is_some());
        assert!(Arch::current().is_some());
        assert!(Platform::current().is_some());
    }

    #[test]
    fn test_platform_target_triples() {
        let cases = [
            (
                Platform {
                    arch: Arch::Aarch64,
                    os: Os::Macos,
                },
                "aarch64-apple-darwin",
            ),
            (
                Platform {
                    arch: Arch::X86_64,
                    os: Os::Macos,
                },
                "x86_64-apple-darwin",
            ),
            (
                Platform {
                    arch: Arch::Aarch64,
                    os: Os::Linux,
                },
                "aarch64-unknown-linux-gnu",
            ),
            (
                Platform {
                    arch: Arch::X86_64,
                    os: Os::Linux,
                },
                "x86_64-unknown-linux-gnu",
            ),
            (
                Platform {
                    arch: Arch::Aarch64,
                    os: Os::Windows,
                },
                "aarch64-pc-windows-msvc",
            ),
            (
                Platform {
                    arch: Arch::X86_64,
                    os: Os::Windows,
                },
                "x86_64-pc-windows-msvc",
            ),
        ];

        for (platform, expected_triple) in cases {
            assert_eq!(platform.target_triple(), expected_triple);
        }
    }

    #[test]
    fn test_match_platform_asset_exact_triple() {
        let platform = Platform {
            arch: Arch::Aarch64,
            os: Os::Macos,
        };
        let assets = vec![
            "rho-v1.0.0-x86_64-apple-darwin.tar.gz".to_string(),
            "rho-v1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
            "rho-v1.0.0-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "rho-v1.0.0-aarch64-apple-darwin.tar.gz");
    }

    #[test]
    fn test_match_platform_asset_fallback_hints() {
        let platform = Platform {
            arch: Arch::Aarch64,
            os: Os::Macos,
        };
        let assets = vec!["rho-darwin-x64.zip".to_string(), "RHO-DARWIN-ARM64.zip".to_string()];
        let matched = match_platform_asset(&platform, &assets).unwrap();
        assert_eq!(matched, "RHO-DARWIN-ARM64.zip");
    }

    #[test]
    fn test_match_platform_asset_no_match_and_empty() {
        let platform = Platform {
            arch: Arch::X86_64,
            os: Os::Linux,
        };
        let assets = vec!["rho-freebsd-x64.tar.gz".to_string()];
        let err = match_platform_asset(&platform, &assets).unwrap_err();
        match err {
            PlatformMatchError::NoMatchingAsset {
                target_triple,
                available,
            } => {
                assert_eq!(target_triple, "x86_64-unknown-linux-gnu");
                assert_eq!(available, assets);
            }
        }

        let empty_assets: Vec<String> = Vec::new();
        let err = match_platform_asset(&platform, &empty_assets).unwrap_err();
        assert!(matches!(err, PlatformMatchError::NoMatchingAsset { .. }));
    }
}
