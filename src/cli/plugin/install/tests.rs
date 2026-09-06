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

async fn spawn_install_download_server(asset_name: String, asset: Arc<Vec<u8>>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"{asset_name}","browser_download_url":"http://{addr}/download/{asset_name}"}}]}}"#
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

async fn spawn_single_json_server(body: &'static str) -> std::net::SocketAddr {
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

#[tokio::test]
async fn test_install_plugin_success_tar_gz() {
    let triple = Platform::current().unwrap().target_triple();
    let asset_name = format!("rho-plugin-sample-{triple}.tar.gz");
    let asset = Arc::new(create_tar_gz(&[("rho-plugin-sample", b"sample-binary-payload")]));
    let addr = spawn_install_download_server(asset_name, asset).await;

    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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

    assert_eq!(
        (result.name.as_str(), result.version.as_str()),
        ("rho-plugin-sample", "v1.2.0")
    );
    assert_eq!(result.binary_path, bin_dir.path().join("rho-plugin-sample"));
    assert_eq!(std::fs::read(&result.binary_path).unwrap(), b"sample-binary-payload");
}

#[tokio::test]
async fn test_install_duplicate_plugin_fails_without_force() {
    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let plugins = BTreeMap::from([(
        "rho-plugin-sample".to_string(),
        PluginConfig {
            command: Some("rho-plugin-sample".to_string()),
            ..Default::default()
        },
    )]);
    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: None,
    };
    assert!(matches!(
        install_plugin(ctx, "sample").await.unwrap_err(),
        InstallError::Duplicate(_)
    ));
}

#[tokio::test]
async fn test_install_existing_binary_fails_without_force() {
    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    match install_plugin(ctx, "sample").await.unwrap_err() {
        InstallError::BinaryAlreadyExists(path) => assert_eq!(path, binary_file),
        other => panic!("expected BinaryAlreadyExists, got {:?}", other),
    }
}

#[tokio::test]
async fn test_install_no_matching_asset_fails() {
    let body = r#"{"tag_name":"v1.0.0","assets":[{"name":"incompatible-platform.deb","browser_download_url":"http://example.com/asset"}]}"#;
    let addr = spawn_single_json_server(body).await;

    let (config_dir, bin_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let plugins = BTreeMap::new();
    let github_client = GitHubClient::with_base_url(format!("http://{addr}"));
    let ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &plugins,
        cargo_bin_dir: bin_dir.path(),
        force: false,
        github_client: Some(&github_client),
    };
    assert!(matches!(
        install_plugin(ctx, "sample").await.unwrap_err(),
        InstallError::PlatformMatch(_)
    ));
}
