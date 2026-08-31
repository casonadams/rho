use super::*;
use async_trait::async_trait;
use rho_core::config::PluginConfig;
use rho_core::error::AppError;
use rho_sdk::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PLUGIN_PROTOCOL_VERSION, ValidatedManifest,
};

struct Validator(BTreeMap<String, ValidatedManifest>);

#[async_trait]
impl PluginValidator for Validator {
    async fn validate(&self, executable: &Path) -> Result<ValidatedManifest> {
        executable
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| self.0.get(name))
            .cloned()
            .ok_or_else(|| AppError::Plugin("invalid".to_string()))
    }
}

fn manifest(name: &str, capability: &str, replaces: Option<&str>) -> ValidatedManifest {
    CapabilityManifest {
        plugin_id: name.parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: capability.parse().unwrap(),
            replaces: replaces.map(|value| value.parse().unwrap()),
        }],
    }
    .validate()
    .unwrap()
}

fn executable(path: &Path) {
    std::fs::write(path, b"fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test]
async fn inspection_reports_builtins_override_and_ignored_candidate() {
    let root = std::env::temp_dir().join(format!("rho_inspect_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("plugins")).unwrap();
    let configured_path = root.join("container");
    executable(&configured_path);
    executable(&root.join("plugins/rho-plugin-ignored"));
    let mut config = Config {
        config_dir: root.clone(),
        ..Config::default()
    };
    config.plugins.insert(
        "container".to_string(),
        PluginConfig {
            path: configured_path,
            package: None,
            replaces: ["tool:bash".parse().unwrap()].into_iter().collect(),
            ..Default::default()
        },
    );
    let validator = Validator(BTreeMap::from([(
        "container".to_string(),
        manifest("container", "tool:container-bash", Some("tool:bash")),
    )]));
    let inspection = inspect_with(&config, None, &validator).await.unwrap();
    assert_eq!(inspection.configured[0].status, "active");
    assert_eq!(
        inspection.resolution.active[&"tool:bash".parse().unwrap()]
            .plugin_id
            .as_str(),
        "container"
    );
    let rendered = inspection.render();
    assert!(rendered.contains("tool:bash"));
    assert!(rendered.contains("replacing tool:bash"));
    assert!(rendered.contains("rho-plugin-ignored [unconfigured; ignored]"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn invalid_configured_plugin_is_inactive_without_hiding_builtins() {
    let root = std::env::temp_dir().join(format!("rho_inspect_invalid_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("invalid");
    executable(&path);
    let mut config = Config {
        config_dir: root.clone(),
        ..Config::default()
    };
    config.plugins.insert(
        "invalid".to_string(),
        PluginConfig {
            path,
            package: None,
            replaces: Default::default(),
            ..Default::default()
        },
    );
    let inspection = inspect_with(&config, None, &Validator(BTreeMap::new())).await.unwrap();
    assert!(inspection.configured[0].status.contains("protocol validation failed"));
    let bash = "tool:bash".parse().unwrap();
    assert_eq!(inspection.resolution.active[&bash].plugin_id.as_str(), "rho.builtin");
    assert!(inspection.render_capability(&bash).contains("BuiltIn"));
    std::fs::remove_dir_all(root).unwrap();
}
