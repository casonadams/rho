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
async fn test_self_update_already_up_to_date() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"tag_name":"v0.3.0","assets":[]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });

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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let platform = Platform::current().unwrap();
    let triple = platform.target_triple();
    let asset_name = format!("rho-{triple}.tar.gz");
    let asset_bytes = create_tar_gz(&[("rho", b"new-rho-binary")]);
    let shared_asset = Arc::new(asset_bytes);

    let asset_clone = shared_asset.clone();
    let asset_name_clone = asset_name.clone();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = format!(
                r#"{{"tag_name":"v0.4.0","assets":[{{"name":"{asset_name_clone}","browser_download_url":"http://{addr}/download/{asset_name_clone}"}}]}}"#
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

    match status {
        SelfUpdateStatus::Updated {
            old_version,
            new_version,
            binary_path,
        } => {
            assert_eq!(old_version, "v0.3.0");
            assert_eq!(new_version, "v0.4.0");
            assert_eq!(binary_path, fake_exe);
        }
        other => panic!("expected Updated, got {:?}", other),
    }

    assert_eq!(std::fs::read(&fake_exe).unwrap(), b"new-rho-binary");
}
