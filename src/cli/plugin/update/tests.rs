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
async fn test_update_single_plugin_already_up_to_date() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"tag_name":"v1.0.0","assets":[]}"#;
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
    let binary_path = bin_dir.path().join("rho-plugin-sample");
    std::fs::write(&binary_path, b"existing-bin").unwrap();

    let plugin_cfg = PluginConfig {
        version: Some("v1.0.0".to_string()),
        command: Some("rho-plugin-sample".to_string()),
        ..Default::default()
    };
    let client = GitHubClient::with_base_url(format!("http://{addr}"));

    let ctx = UpdatePluginContext {
        config_dir: config_dir.path(),
        cargo_bin_dir: bin_dir.path(),
        client: &client,
    };
    let status = update_single_plugin(ctx, "rho-plugin-sample", &plugin_cfg)
        .await
        .unwrap();

    assert_eq!(
        status,
        PluginUpdateStatus::AlreadyUpToDate {
            version: "v1.0.0".to_string()
        }
    );
    assert_eq!(std::fs::read(&binary_path).unwrap(), b"existing-bin");
}

#[tokio::test]
async fn test_update_single_plugin_success() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let platform = Platform::current().unwrap();
    let triple = platform.target_triple();
    let asset_name = format!("rho-plugin-sample-{triple}.tar.gz");
    let asset_bytes = create_tar_gz(&[("rho-plugin-sample", b"updated-sample-payload")]);
    let shared_asset = Arc::new(asset_bytes);

    let asset_clone = shared_asset.clone();
    let asset_name_clone = asset_name.clone();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = format!(
                r#"{{"tag_name":"v2.0.0","assets":[{{"name":"{asset_name_clone}","browser_download_url":"http://{addr}/download/{asset_name_clone}"}}]}}"#
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
    let binary_path = bin_dir.path().join("rho-plugin-sample");
    std::fs::write(&binary_path, b"old-payload").unwrap();

    let plugin_cfg = PluginConfig {
        version: Some("v1.0.0".to_string()),
        command: Some("rho-plugin-sample".to_string()),
        ..Default::default()
    };
    let client = GitHubClient::with_base_url(format!("http://{addr}"));

    let ctx = UpdatePluginContext {
        config_dir: config_dir.path(),
        cargo_bin_dir: bin_dir.path(),
        client: &client,
    };
    let status = update_single_plugin(ctx, "rho-plugin-sample", &plugin_cfg)
        .await
        .unwrap();

    match status {
        PluginUpdateStatus::Updated {
            old_version,
            new_version,
            binary_path: p,
        } => {
            assert_eq!(old_version, "v1.0.0");
            assert_eq!(new_version, "v2.0.0");
            assert_eq!(p, binary_path);
        }
        other => panic!("expected Updated, got {:?}", other),
    }

    assert_eq!(std::fs::read(&binary_path).unwrap(), b"updated-sample-payload");
    let config_text = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(config_text.contains("version = \"v2.0.0\""));
}

#[tokio::test]
async fn test_handle_update_plugin_not_configured() {
    let config = Config::default();
    let err = handle_update_plugin(&config, "nonexistent").await.unwrap_err();
    assert!(err.to_string().contains("not configured"));
}
