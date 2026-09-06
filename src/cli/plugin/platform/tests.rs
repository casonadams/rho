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
