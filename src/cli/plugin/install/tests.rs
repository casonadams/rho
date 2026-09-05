use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn create_tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_path(name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append(&header, *content).unwrap();
        }
        tar.finish().unwrap();
    }
    gz.finish().unwrap()
}

#[tokio::test]
async fn test_install_plugin_success_tar_gz() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let platform = Platform::current().unwrap();
    let triple = platform.target_triple();
    let asset_name = format!("rho-plugin-sample-{triple}.tar.gz");
    let asset_bytes = create_tar_gz(&[("rho-plugin-sample", b"sample-binary-payload")]);
    let shared_asset = Arc::new(asset_bytes);

    let asset_clone = shared_asset.clone();
    let asset_name_clone = asset_name.clone();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = format!(
                r#"{{"tag_name":"v1.2.0","assets":[{{"name":"{asset_name_clone}","browser_download_url":"http://{addr}/download/{asset_name_clone}"}}]}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }

        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                asset_clone.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.write_all(&asset_clone).await;
        }
    });

    let config_dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let plugins = BTreeMap::new();
    let github_client = GitHubClient::with_base_url(format!("http://{addr}"));

    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: Some(&github_client),
    };

    let result = install_plugin(ctx, "sample").await.unwrap();

    assert_eq!(result.name, "rho-plugin-sample");
    assert_eq!(result.version, "v1.2.0");
    assert_eq!(result.binary_path, bin_dir.path().join("rho-plugin-sample"));
    assert!(result.binary_path.is_file());
    assert_eq!(std::fs::read(&result.binary_path).unwrap(), b"sample-binary-payload");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&result.binary_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o755);
    }

    let config_content = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(config_content.contains("[plugins.rho-plugin-sample]"));
}

#[tokio::test]
async fn test_install_duplicate_plugin_fails_without_force() {
    let config_dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "rho-plugin-sample".to_string(),
        PluginConfig {
            command: Some("rho-plugin-sample".to_string()),
            ..Default::default()
        },
    );

    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: None,
    };

    let err = install_plugin(ctx, "sample").await.unwrap_err();
    assert!(matches!(err, InstallError::Duplicate(_)));
}

#[tokio::test]
async fn test_install_existing_binary_fails_without_force() {
    let config_dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let binary_file = bin_dir.path().join("rho-plugin-sample");
    std::fs::write(&binary_file, b"existing").unwrap();

    let plugins = BTreeMap::new();
    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: None,
    };

    let err = install_plugin(ctx, "sample").await.unwrap_err();
    match err {
        InstallError::BinaryAlreadyExists(path) => {
            assert_eq!(path, binary_file);
        }
        other => panic!("expected BinaryAlreadyExists, got {:?}", other),
    }
}

#[tokio::test]
async fn test_install_no_matching_asset_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"tag_name":"v1.0.0","assets":[{"name":"incompatible-platform.deb","browser_download_url":"http://example.com/asset"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });

    let config_dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let plugins = BTreeMap::new();
    let github_client = GitHubClient::with_base_url(format!("http://{addr}"));

    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: Some(&github_client),
    };

    let err = install_plugin(ctx, "sample").await.unwrap_err();
    assert!(matches!(err, InstallError::PlatformMatch(_)));
}
