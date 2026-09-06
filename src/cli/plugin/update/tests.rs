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

async fn spawn_mock_github_single(body: &'static str) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });
    addr
}

async fn serve_raw_bytes(mut stream: tokio::net::TcpStream, bytes: &[u8]) {
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf).await;
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.write_all(bytes).await;
}

async fn serve_raw_json(mut stream: tokio::net::TcpStream, body: &str) {
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf).await;
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}

async fn spawn_update_download_server(asset_name: String, asset: Arc<Vec<u8>>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = format!(
        r#"{{"tag_name":"v2.0.0","assets":[{{"name":"{asset_name}","browser_download_url":"http://{addr}/download/{asset_name}"}}]}}"#
    );
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            serve_raw_json(stream, &body).await;
        }
        if let Ok((stream, _)) = listener.accept().await {
            serve_raw_bytes(stream, &asset).await;
        }
    });
    addr
}

#[tokio::test]
async fn test_update_single_plugin_already_up_to_date() {
    let addr = spawn_mock_github_single(r#"{"tag_name":"v1.0.0","assets":[]}"#).await;
    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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

fn assert_update_success(status: &PluginUpdateStatus, binary_path: &std::path::Path, config_dir: &std::path::Path) {
    assert_eq!(
        *status,
        PluginUpdateStatus::Updated {
            old_version: "v1.0.0".to_string(),
            new_version: "v2.0.0".to_string(),
            binary_path: binary_path.to_path_buf(),
        }
    );
    assert_eq!(std::fs::read(binary_path).unwrap(), b"updated-sample-payload");
    assert!(
        std::fs::read_to_string(config_dir.join("config.toml"))
            .unwrap()
            .contains("version = \"v2.0.0\"")
    );
}

#[tokio::test]
async fn test_update_single_plugin_success() {
    let triple = Platform::current().unwrap().target_triple();
    let asset_name = format!("rho-plugin-sample-{triple}.tar.gz");
    let asset = Arc::new(create_tar_gz(&[("rho-plugin-sample", b"updated-sample-payload")]));
    let addr = spawn_update_download_server(asset_name, asset).await;

    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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

    assert_update_success(&status, &binary_path, config_dir.path());
}

#[tokio::test]
async fn test_handle_update_plugin_not_configured() {
    let config = Config::default();
    let err = handle_update_plugin(&config, "nonexistent").await.unwrap_err();
    assert!(err.to_string().contains("not configured"));
}
