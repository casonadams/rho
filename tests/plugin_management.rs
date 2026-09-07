use rho::cli::plugin::github::GitHubClient;
use rho::cli::plugin::install::{InstallPluginContext, install_plugin};
use rho::cli::plugin::listing::collect_plugin_listings;
use rho::cli::plugin::paths::PluginEnvironment;
use rho::cli::plugin::platform::Platform;
use rho::cli::plugin::remove::{RemovePluginContext, remove_plugin};
use rho::cli::plugin::update::{PluginUpdateStatus, UpdatePluginContext, update_single_plugin};
use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
use std::path::Path;
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

fn release_body(turn: usize, addr: std::net::SocketAddr, asset: &str) -> String {
    let (ver, dl_url) = if turn <= 1 {
        ("v1.0.0", format!("http://{addr}/download/v1.tar.gz"))
    } else {
        ("v2.0.0", format!("http://{addr}/download/v2.tar.gz"))
    };
    format!(r#"{{"tag_name":"{ver}","assets":[{{"name":"{asset}","browser_download_url":"{dl_url}"}}]}}"#)
}

async fn serve_plugin_bytes(stream: &mut tokio::net::TcpStream, bytes: &[u8]) {
    let _ = stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .as_bytes(),
        )
        .await;
    let _ = stream.write_all(bytes).await;
}

async fn serve_download(stream: &mut tokio::net::TcpStream, req: &str, v1: &[u8], v2: &[u8]) -> bool {
    if req.contains("/download/v1.tar.gz") {
        serve_plugin_bytes(stream, v1).await;
        return true;
    }
    if req.contains("/download/v2.tar.gz") {
        serve_plugin_bytes(stream, v2).await;
        return true;
    }
    false
}

async fn serve_release_metadata(
    stream: &mut tokio::net::TcpStream,
    turn: usize,
    addr: std::net::SocketAddr,
    asset: &str,
) {
    let body = release_body(turn, addr, asset);
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}

async fn spawn_github_release_server(asset_name: String) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let v1 = Arc::new(create_tar_gz(&[("rho-plugin-e2e", b"v1-executable-bytes")]));
    let v2 = Arc::new(create_tar_gz(&[("rho-plugin-e2e", b"v2-executable-bytes")]));

    tokio::spawn(async move {
        let mut turn = 0;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            if serve_download(&mut stream, &req, &v1, &v2).await {
                continue;
            }
            if req.contains("/releases/latest") {
                turn += 1;
                serve_release_metadata(&mut stream, turn, addr, &asset_name).await;
            }
        }
    });
    addr
}

async fn step_install(config_dir: &Path, bin_dir: &Path, gh: &GitHubClient) {
    let empty = BTreeMap::new();
    let ctx = InstallPluginContext {
        config_dir,
        plugins: &empty,
        cargo_bin_dir: bin_dir,
        force: false,
        github_client: Some(gh),
    };
    let res = install_plugin(ctx, "e2e").await.unwrap();
    assert_eq!((res.name.as_str(), res.version.as_str()), ("rho-plugin-e2e", "v1.0.0"));
    assert_eq!(
        std::fs::read(bin_dir.join("rho-plugin-e2e")).unwrap(),
        b"v1-executable-bytes"
    );
}

async fn step_update(config_dir: &Path, bin_dir: &Path, gh: &GitHubClient, plugins: &BTreeMap<String, PluginConfig>) {
    let ctx = UpdatePluginContext {
        config_dir,
        cargo_bin_dir: bin_dir,
        client: gh,
    };
    let res = update_single_plugin(ctx, "rho-plugin-e2e", &plugins["rho-plugin-e2e"])
        .await
        .unwrap();
    match res {
        PluginUpdateStatus::Updated {
            old_version,
            new_version,
            ..
        } => {
            assert_eq!((old_version.as_str(), new_version.as_str()), ("v1.0.0", "v2.0.0"));
        }
        other => panic!("expected Updated, got {:?}", other),
    }
    assert_eq!(
        std::fs::read(bin_dir.join("rho-plugin-e2e")).unwrap(),
        b"v2-executable-bytes"
    );
}

async fn step_remove(config_dir: &Path, bin_dir: &Path, plugins: &BTreeMap<String, PluginConfig>) {
    let ctx = RemovePluginContext {
        config_dir,
        plugins,
        keep_binary: false,
        cargo_bin_dir: Some(bin_dir),
        home_dir: None,
    };
    let res = remove_plugin(ctx, "e2e").await.unwrap();
    assert_eq!(res.name, "rho-plugin-e2e");
    assert!(!bin_dir.join("rho-plugin-e2e").exists());
}

fn seed_e2e_plugins() -> BTreeMap<String, PluginConfig> {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "rho-plugin-e2e".to_string(),
        PluginConfig {
            command: Some("rho-plugin-e2e".to_string()),
            version: Some("v1.0.0".to_string()),
            enabled: true,
            ..Default::default()
        },
    );
    plugins
}

#[tokio::test]
async fn test_plugin_management_lifecycle_end_to_end() {
    let triple = Platform::current().unwrap().target_triple();
    let addr = spawn_github_release_server(format!("rho-plugin-e2e-{triple}.tar.gz")).await;

    let config_dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let gh = GitHubClient::with_base_url(format!("http://{addr}"));

    step_install(config_dir.path(), bin_dir.path(), &gh).await;
    let plugins = seed_e2e_plugins();

    let env = PluginEnvironment {
        cargo_bin_dir: Some(bin_dir.path()),
        home_dir: None,
    };
    let listings = collect_plugin_listings(&plugins, env);
    assert_eq!((listings.len(), listings[0].name.as_str()), (1, "rho-plugin-e2e"));

    step_update(config_dir.path(), bin_dir.path(), &gh, &plugins).await;
    step_remove(config_dir.path(), bin_dir.path(), &plugins).await;
}
