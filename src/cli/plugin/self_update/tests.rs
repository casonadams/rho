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

async fn spawn_self_update_single(body: &'static str) -> std::net::SocketAddr {
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

async fn spawn_self_update_download(asset_name: String, asset: Arc<Vec<u8>>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = format!(
        r#"{{"tag_name":"v0.4.0","assets":[{{"name":"{asset_name}","browser_download_url":"http://{addr}/download/{asset_name}"}}]}}"#
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
async fn test_self_update_already_up_to_date() {
    let addr = spawn_self_update_single(r#"{"tag_name":"v0.3.0","assets":[]}"#).await;
    let temp_dir = tempfile::tempdir().unwrap();
    let fake_exe = temp_dir.path().join("rho");
    std::fs::write(&fake_exe, b"old-binary").unwrap();

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let status = self_update(SelfUpdateContext {
        current_exe: &fake_exe,
        current_version: "v0.3.0",
        github_client: Some(&client),
    })
    .await
    .unwrap();

    assert_eq!(
        status,
        SelfUpdateStatus::AlreadyUpToDate {
            version: "v0.3.0".to_string()
        }
    );
    assert_eq!(std::fs::read(&fake_exe).unwrap(), b"old-binary");
}

#[tokio::test]
async fn test_self_update_success() {
    let triple = Platform::current().unwrap().target_triple();
    let asset_name = format!("rho-{triple}.tar.gz");
    let asset = Arc::new(create_tar_gz(&[("rho", b"new-rho-binary")]));
    let addr = spawn_self_update_download(asset_name, asset).await;

    let temp_dir = tempfile::tempdir().unwrap();
    let fake_exe = temp_dir.path().join("rho");
    std::fs::write(&fake_exe, b"old-rho-binary").unwrap();

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let status = self_update(SelfUpdateContext {
        current_exe: &fake_exe,
        current_version: "v0.3.0",
        github_client: Some(&client),
    })
    .await
    .unwrap();

    assert_eq!(
        status,
        SelfUpdateStatus::Updated {
            old_version: "v0.3.0".to_string(),
            new_version: "v0.4.0".to_string(),
            binary_path: fake_exe.clone()
        }
    );
    assert_eq!(std::fs::read(&fake_exe).unwrap(), b"new-rho-binary");
}
