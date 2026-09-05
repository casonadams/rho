use super::*;

#[test]
fn test_target_triples() {
    assert_eq!(
        Platform {
            os: Os::Macos,
            arch: Arch::Aarch64
        }
        .target_triple(),
        "aarch64-apple-darwin"
    );
    assert_eq!(
        Platform {
            os: Os::Macos,
            arch: Arch::X86_64
        }
        .target_triple(),
        "x86_64-apple-darwin"
    );
    assert_eq!(
        Platform {
            os: Os::Linux,
            arch: Arch::X86_64
        }
        .target_triple(),
        "x86_64-unknown-linux-gnu"
    );
    assert_eq!(
        Platform {
            os: Os::Linux,
            arch: Arch::Aarch64
        }
        .target_triple(),
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(
        Platform {
            os: Os::Windows,
            arch: Arch::X86_64
        }
        .target_triple(),
        "x86_64-pc-windows-msvc"
    );
    assert_eq!(
        Platform {
            os: Os::Windows,
            arch: Arch::Aarch64
        }
        .target_triple(),
        "aarch64-pc-windows-msvc"
    );
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
