use rho::cli::plugin::github::GitHubClient;
use rho::cli::plugin::install::{InstallPluginContext, install_plugin};
use rho::cli::plugin::listing::collect_plugin_listings;
use rho::cli::plugin::paths::PluginEnvironment;
use rho::cli::plugin::platform::Platform;
use rho::cli::plugin::remove::{RemovalArtifactStatus, RemovePluginContext, remove_plugin};
use rho::cli::plugin::update::{PluginUpdateStatus, UpdatePluginContext, update_single_plugin};
use rho_harness_core::config::PluginConfig;
use std::collections::BTreeMap;
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
async fn test_plugin_management_lifecycle_end_to_end() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let platform = Platform::current().unwrap();
    let triple = platform.target_triple();
    let asset_name = format!("rho-plugin-e2e-{triple}.tar.gz");

    let v1_tar = Arc::new(create_tar_gz(&[("rho-plugin-e2e", b"v1-executable-bytes")]));
    let v2_tar = Arc::new(create_tar_gz(&[("rho-plugin-e2e", b"v2-executable-bytes")]));

    let v1_clone = v1_tar.clone();
    let v2_clone = v2_tar.clone();
    let asset_name_clone = asset_name.clone();

    tokio::spawn(async move {
        let mut turn = 0;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.contains("/releases/latest") {
                turn += 1;
                let (ver, dl_url) = if turn <= 1 {
                    ("v1.0.0", format!("http://{addr}/download/v1.tar.gz"))
                } else {
                    ("v2.0.0", format!("http://{addr}/download/v2.tar.gz"))
                };
                let body = format!(
                    r#"{{"tag_name":"{ver}","assets":[{{"name":"{asset_name_clone}","browser_download_url":"{dl_url}"}}]}}"#
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes()).await;
            } else if req.contains("/download/v1.tar.gz") {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    v1_clone.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.write_all(&v1_clone).await;
            } else if req.contains("/download/v2.tar.gz") {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    v2_clone.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.write_all(&v2_clone).await;
            }
        }
    });

    let config_dir = tempfile::tempdir().unwrap();
    let cargo_bin_dir = tempfile::tempdir().unwrap();
    let github_client = GitHubClient::with_base_url(format!("http://{addr}"));

    let empty_plugins = BTreeMap::new();
    let install_ctx = InstallPluginContext {
        config_dir: config_dir.path(),
        plugins: &empty_plugins,
        cargo_bin_dir: cargo_bin_dir.path(),
        force: false,
        github_client: Some(&github_client),
    };

    let install_res = install_plugin(install_ctx, "e2e").await.unwrap();
    assert_eq!(install_res.name, "rho-plugin-e2e");
    assert_eq!(install_res.version, "v1.0.0");
    let binary_path = cargo_bin_dir.path().join("rho-plugin-e2e");
    assert_eq!(install_res.binary_path, binary_path);
    assert_eq!(std::fs::read(&binary_path).unwrap(), b"v1-executable-bytes");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&binary_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o755);
    }

    let config_content = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(config_content.contains("[plugins.rho-plugin-e2e]"));

    let mut loaded_plugins = BTreeMap::new();
    loaded_plugins.insert(
        "rho-plugin-e2e".to_string(),
        PluginConfig {
            command: Some("rho-plugin-e2e".to_string()),
            version: Some("v1.0.0".to_string()),
            enabled: true,
            ..Default::default()
        },
    );

    let listings = collect_plugin_listings(
        &loaded_plugins,
        PluginEnvironment {
            cargo_bin_dir: Some(cargo_bin_dir.path()),
            home_dir: None,
        },
    );
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].name, "rho-plugin-e2e");
    assert_eq!(listings[0].managed, "cargo-bin");
    assert!(listings[0].status.contains("Installed (active)"));

    let update_ctx = UpdatePluginContext {
        config_dir: config_dir.path(),
        cargo_bin_dir: cargo_bin_dir.path(),
        client: &github_client,
    };
    let update_res = update_single_plugin(update_ctx, "rho-plugin-e2e", &loaded_plugins["rho-plugin-e2e"])
        .await
        .unwrap();

    match update_res {
        PluginUpdateStatus::Updated {
            old_version,
            new_version,
            binary_path: p,
        } => {
            assert_eq!(old_version, "v1.0.0");
            assert_eq!(new_version, "v2.0.0");
            assert_eq!(p, binary_path);
        }
        other => panic!("expected Updated status, got {:?}", other),
    }
    assert_eq!(std::fs::read(&binary_path).unwrap(), b"v2-executable-bytes");

    let remove_ctx = RemovePluginContext {
        config_dir: config_dir.path(),
        plugins: &loaded_plugins,
        keep_binary: false,
        cargo_bin_dir: Some(cargo_bin_dir.path()),
        home_dir: None,
    };
    let remove_res = remove_plugin(remove_ctx, "e2e").await.unwrap();
    assert_eq!(remove_res.name, "rho-plugin-e2e");
    assert_eq!(
        remove_res.artifact_status,
        RemovalArtifactStatus::Deleted(binary_path.clone())
    );
    assert!(!binary_path.exists());

    let remaining_config = std::fs::read_to_string(config_dir.path().join("config.toml")).unwrap();
    assert!(!remaining_config.contains("rho-plugin-e2e"));
}
