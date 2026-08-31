use super::*;
use async_trait::async_trait;
use rho_core::config::{Config, PluginConfig};
use rho_core::error::{AppError, Result};
use rho_sdk::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PLUGIN_PROTOCOL_VERSION, PluginId,
    ValidatedManifest,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn manifest(replacement: Option<&str>) -> ValidatedManifest {
    CapabilityManifest {
        plugin_id: PluginId::new("fixture").unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: vec![CapabilityDeclaration {
            id: "tool:fixture".parse().unwrap(),
            replaces: replacement.map(|value| value.parse().unwrap()),
        }],
    }
    .validate()
    .unwrap()
}

struct MockCargo {
    bin: PathBuf,
    fail_install: bool,
    fail_uninstall: bool,
    events: Mutex<Vec<String>>,
}

impl CargoRunner for MockCargo {
    fn install(&self, package: &str) -> Result<()> {
        self.events.lock().unwrap().push(format!("install:{package}"));
        if self.fail_install {
            return Err(AppError::Plugin("install failed".to_string()));
        }
        let path = manager::executable_path(&self.bin, manager::executable_names(package).first().unwrap());
        std::fs::write(&path, b"fixture")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }

    fn uninstall(&self, package: &str) -> Result<()> {
        self.events.lock().unwrap().push(format!("uninstall:{package}"));
        if self.fail_uninstall {
            Err(AppError::Plugin("uninstall failed".to_string()))
        } else {
            Ok(())
        }
    }
}

struct MockValidator(Result<ValidatedManifest>);

#[async_trait]
impl PluginValidator for MockValidator {
    async fn validate(&self, _executable: &Path) -> Result<ValidatedManifest> {
        self.0
            .as_ref()
            .cloned()
            .map_err(|error| AppError::Plugin(error.to_string()))
    }
}

fn manager(
    fail_install: bool,
    fail_validation: bool,
    fail_uninstall: bool,
) -> (PathBuf, PluginManager<MockCargo, MockValidator>) {
    let root = std::env::temp_dir().join(format!("rho_manager_{}", uuid::Uuid::new_v4()));
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let validator = if fail_validation {
        MockValidator(Err(AppError::Plugin("validation failed".to_string())))
    } else {
        MockValidator(Ok(manifest(None)))
    };
    let manager = PluginManager::new(
        PluginManagerPaths {
            config_dir: root.clone(),
            cargo_bin: bin.clone(),
        },
        MockCargo {
            bin,
            fail_install,
            fail_uninstall,
            events: Mutex::new(Vec::new()),
        },
        validator,
    );
    (root, manager)
}

#[tokio::test]
async fn successful_install_records_package_executable_and_replacements() {
    let (root, manager) = manager(false, false, false);
    let installed = manager.install("rho-plugin-fixture", BTreeSet::new()).await.unwrap();
    assert_eq!(installed.name, "fixture");
    let config: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap();
    assert_eq!(
        config["plugins"]["fixture"]["package"].as_str(),
        Some("rho-plugin-fixture")
    );
    assert!(installed.executable.is_absolute());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cargo_install_failure_writes_nothing() {
    let (root, manager) = manager(true, false, false);
    assert!(manager.install("rho-plugin-fixture", BTreeSet::new()).await.is_err());
    assert!(!root.join("config.toml").exists());
    assert_eq!(*manager.cargo.events.lock().unwrap(), ["install:rho-plugin-fixture"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn failed_validation_writes_nothing_and_rolls_back_new_install() {
    let (root, manager) = manager(false, true, false);
    assert!(manager.install("rho-plugin-fixture", BTreeSet::new()).await.is_err());
    assert!(!root.join("config.toml").exists());
    assert_eq!(
        *manager.cargo.events.lock().unwrap(),
        ["install:rho-plugin-fixture", "uninstall:rho-plugin-fixture"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn failed_validation_does_not_uninstall_a_preexisting_executable() {
    let (root, manager) = manager(false, true, false);
    let executable = manager::executable_path(&manager.cargo.bin, "rho-plugin-fixture");
    std::fs::write(&executable, b"existing").unwrap();
    assert!(manager.install("rho-plugin-fixture", BTreeSet::new()).await.is_err());
    assert_eq!(*manager.cargo.events.lock().unwrap(), ["install:rho-plugin-fixture"]);
    assert!(!root.join("config.toml").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn rollback_failure_is_reported_without_writing_configuration() {
    let (root, manager) = manager(false, true, true);
    let error = manager
        .install("rho-plugin-fixture", BTreeSet::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("rollback"));
    assert!(!root.join("config.toml").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn replacement_authorization_must_exactly_match_manifest() {
    let (root, mut manager) = manager(false, false, false);
    manager.validator = MockValidator(Ok(manifest(Some("tool:bash"))));
    assert!(manager.install("rho-plugin-fixture", BTreeSet::new()).await.is_err());
    assert!(!root.join("config.toml").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn explicitly_authorized_replacement_is_recorded() {
    let (root, mut manager) = manager(false, false, false);
    manager.validator = MockValidator(Ok(manifest(Some("tool:bash"))));
    let replacements = ["tool:bash".parse().unwrap()].into_iter().collect();
    manager.install("rho-plugin-fixture", replacements).await.unwrap();
    let config: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap();
    assert_eq!(config["plugins"]["fixture"]["replaces"][0].as_str(), Some("tool:bash"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removal_updates_configuration_before_cargo_uninstall_failure() {
    let (root, manager) = manager(false, false, true);
    Config::add_plugin(
        &root,
        "fixture",
        PluginConfig {
            path: root.join("bin/rho-plugin-fixture"),
            package: Some("rho-plugin-fixture".to_string()),
            replaces: BTreeSet::new(),
            ..Default::default()
        },
    )
    .unwrap();
    let error = manager.remove("fixture").unwrap_err().to_string();
    assert!(error.contains("declaration was removed"));
    let config: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("config.toml")).unwrap()).unwrap();
    assert!(config["plugins"].as_table().unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_local_plugin_does_not_delete_executable() {
    let (root, manager) = manager(false, false, false);
    let executable = root.join("local-plugin");
    std::fs::write(&executable, b"fixture").unwrap();
    Config::add_plugin(
        &root,
        "fixture",
        PluginConfig {
            path: executable.clone(),
            package: None,
            replaces: BTreeSet::new(),
            ..Default::default()
        },
    )
    .unwrap();
    manager.remove("fixture").unwrap();
    assert!(executable.exists());
    assert!(manager.cargo.events.lock().unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn protocol_validator_negotiates_and_discovers_manifest() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("rho_validator_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("rho-plugin-fixture");
    let manifest = serde_json::to_string(&CapabilityManifest {
        plugin_id: "fixture".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities: Vec::new(),
    })
    .unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nread first\nfirst_id=$(printf '%s' \"$first\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",\"type\":\"terminal_response\",\"result\":{{\"kind\":\"handshake\",\"response\":{{\"selected_version\":1}}}}}}\\n' \"$first_id\"\nread second\nsecond_id=$(printf '%s' \"$second\" | sed -E 's/.*\"request_id\":\"([^\"]+)\".*/\\1/')\nprintf '{{\"protocol_version\":1,\"request_id\":\"%s\",\"type\":\"terminal_response\",\"result\":{{\"kind\":\"discovery\",\"response\":{{\"manifest\":{manifest},\"capabilities\":[]}}}}}}\\n' \"$second_id\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        validate_executable(&script).await.unwrap().plugin_id.as_str(),
        "fixture"
    );
    std::fs::remove_dir_all(root).unwrap();
}
