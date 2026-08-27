use crate::config::{Config, PluginConfig};
use crate::error::{AppError, Result};
use crate::plugin::capability::{CapabilityId, PLUGIN_PROTOCOL_VERSION, ValidatedManifest};
use crate::plugin::protocol::{
    Envelope, MAX_PROTOCOL_LINE_BYTES, ProtocolMessage, RequestId, TerminalResult, decode_line, encode_line,
};
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

const VALIDATION_TIMEOUT: Duration = Duration::from_secs(5);

pub trait CargoRunner: Send + Sync {
    fn install(&self, package: &str) -> Result<()>;
    fn uninstall(&self, package: &str) -> Result<()>;
}

pub struct SystemCargo;

pub fn default_cargo_bin() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(PathBuf::from(root).join("bin"));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".cargo").join("bin"))
        .ok_or_else(|| AppError::Plugin("Cannot determine Cargo installation directory".to_string()))
}

impl CargoRunner for SystemCargo {
    fn install(&self, package: &str) -> Result<()> {
        run_cargo(["install", package])
    }

    fn uninstall(&self, package: &str) -> Result<()> {
        run_cargo(["uninstall", package])
    }
}

fn run_cargo<const N: usize>(arguments: [&str; N]) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(arguments)
        .status()
        .map_err(|error| AppError::Plugin(format!("Failed to run Cargo: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Plugin(format!("Cargo exited with status {status}")))
    }
}

#[async_trait]
pub trait PluginValidator: Send + Sync {
    async fn validate(&self, executable: &Path) -> Result<ValidatedManifest>;
}

pub struct ProtocolPluginValidator;

#[async_trait]
impl PluginValidator for ProtocolPluginValidator {
    async fn validate(&self, executable: &Path) -> Result<ValidatedManifest> {
        validate_executable(executable).await
    }
}

pub struct PluginManagerPaths {
    pub config_dir: PathBuf,
    pub cargo_bin: PathBuf,
}

pub struct PluginManager<R, V> {
    paths: PluginManagerPaths,
    cargo: R,
    validator: V,
}

impl<R, V> PluginManager<R, V>
where
    R: CargoRunner,
    V: PluginValidator,
{
    pub fn new(paths: PluginManagerPaths, cargo: R, validator: V) -> Self {
        Self {
            paths,
            cargo,
            validator,
        }
    }

    pub async fn install(
        &self,
        package: &str,
        authorized_replacements: BTreeSet<CapabilityId>,
    ) -> Result<InstalledPlugin> {
        validate_package(package)?;
        let executable_names = executable_names(package);
        let existed_before = executable_names
            .iter()
            .any(|name| executable_path(&self.paths.cargo_bin, name).is_file());
        self.cargo.install(package)?;
        let newly_installed = !existed_before;
        let outcome = self
            .complete_install(PendingInstall {
                package,
                executable_names: &executable_names,
                authorized_replacements,
            })
            .await;
        match outcome {
            Ok(installed) => Ok(installed),
            Err(error) if newly_installed => match self.cargo.uninstall(package) {
                Ok(()) => Err(error),
                Err(rollback) => Err(AppError::Plugin(format!(
                    "{error}; Cargo uninstall rollback also failed: {rollback}"
                ))),
            },
            Err(error) => Err(error),
        }
    }

    async fn complete_install(&self, pending: PendingInstall<'_>) -> Result<InstalledPlugin> {
        let executable = pending
            .executable_names
            .iter()
            .map(|name| executable_path(&self.paths.cargo_bin, name))
            .find(|path| path.is_file() && executable_is_runnable(path))
            .ok_or_else(|| {
                AppError::Plugin(format!(
                    "Installed package does not provide a runnable {} or {} executable",
                    pending
                        .executable_names
                        .first()
                        .map_or("rho-plugin-<name>", String::as_str),
                    pending.executable_names.get(1).map_or("rho-<name>", String::as_str)
                ))
            })?;
        let executable = std::fs::canonicalize(executable)?;
        let manifest = self.validator.validate(&executable).await?;
        let declared_replacements: BTreeSet<_> = manifest
            .capabilities
            .iter()
            .filter_map(|declaration| declaration.replaces.clone())
            .collect();
        if declared_replacements != pending.authorized_replacements {
            return Err(AppError::Plugin(format!(
                "Authorized replacements do not exactly match plugin metadata: declared [{}], authorized [{}]",
                display_ids(&declared_replacements),
                display_ids(&pending.authorized_replacements)
            )));
        }
        let name = manifest.plugin_id.to_string();
        let plugin = PluginConfig {
            path: executable.clone(),
            package: Some(pending.package.to_string()),
            replaces: pending.authorized_replacements,
        };
        Config::add_plugin(&self.paths.config_dir, &name, plugin)?;
        Ok(InstalledPlugin {
            name,
            executable,
            manifest,
        })
    }

    pub fn remove(&self, name: &str) -> Result<RemovedPlugin> {
        let plugin = Config::remove_plugin(&self.paths.config_dir, name)?;
        if let Some(package) = &plugin.package
            && let Err(error) = self.cargo.uninstall(package)
        {
            return Err(AppError::Plugin(format!(
                "Plugin declaration was removed, but Cargo uninstall failed: {error}"
            )));
        }
        Ok(RemovedPlugin {
            name: name.to_string(),
            path: plugin.path,
            package: plugin.package,
        })
    }
}

struct PendingInstall<'a> {
    package: &'a str,
    executable_names: &'a [String],
    authorized_replacements: BTreeSet<CapabilityId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub name: String,
    pub executable: PathBuf,
    pub manifest: ValidatedManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedPlugin {
    pub name: String,
    pub path: PathBuf,
    pub package: Option<String>,
}

fn validate_package(package: &str) -> Result<()> {
    if package.is_empty()
        || package.len() > 128
        || !package.is_ascii()
        || !package.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::Plugin("Cargo package name is invalid".to_string()));
    }
    Ok(())
}

fn executable_names(package: &str) -> Vec<String> {
    let package = package.replace('_', "-");
    let suffix = package
        .strip_prefix("rho-plugin-")
        .or_else(|| package.strip_prefix("rho-"))
        .unwrap_or(&package)
        .to_string();
    let preferred = package.starts_with("rho-").then_some(package);
    let mut names = Vec::new();
    for name in [
        preferred,
        Some(format!("rho-plugin-{suffix}")),
        Some(format!("rho-{suffix}")),
    ]
    .into_iter()
    .flatten()
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn executable_path(cargo_bin: &Path, name: &str) -> PathBuf {
    cargo_bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn display_ids(ids: &BTreeSet<CapabilityId>) -> String {
    ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(unix)]
fn executable_is_runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_is_runnable(path: &Path) -> bool {
    path.is_file()
}

pub async fn validate_executable(executable: &Path) -> Result<ValidatedManifest> {
    let mut child = tokio::process::Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| AppError::Plugin(format!("Failed to start configured plugin: {error}")))?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Plugin("Configured plugin stdin is unavailable".to_string()))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Plugin("Configured plugin stdout is unavailable".to_string()))?;
    let mut output = BufReader::new(output);

    let validation = tokio::time::timeout(VALIDATION_TIMEOUT, async {
        let handshake_id = RequestId::new("install-handshake").map_err(protocol_error)?;
        let handshake = exchange(
            &mut input,
            &mut output,
            Envelope::new(
                handshake_id,
                ProtocolMessage::HandshakeRequest {
                    supported_versions: vec![PLUGIN_PROTOCOL_VERSION],
                },
            ),
        )
        .await?;
        match handshake.message {
            ProtocolMessage::TerminalResponse {
                result: TerminalResult::Handshake { selected_version },
            } if selected_version == PLUGIN_PROTOCOL_VERSION => {}
            _ => return Err(AppError::Plugin("Configured plugin handshake was invalid".to_string())),
        }

        let discovery_id = RequestId::new("install-discovery").map_err(protocol_error)?;
        let discovery = exchange(
            &mut input,
            &mut output,
            Envelope::new(discovery_id, ProtocolMessage::DiscoveryRequest),
        )
        .await?;
        let ProtocolMessage::TerminalResponse {
            result: TerminalResult::Discovery { manifest },
        } = discovery.message
        else {
            return Err(AppError::Plugin(
                "Configured plugin discovery response was invalid".to_string(),
            ));
        };
        manifest
            .validate()
            .map_err(|_| AppError::Plugin("Configured plugin manifest was invalid".to_string()))
    })
    .await;

    let _ = child.kill().await;
    let _ = child.wait().await;
    validation.map_err(|_| AppError::Plugin("Configured plugin validation timed out".to_string()))?
}

async fn exchange(
    input: &mut tokio::process::ChildStdin,
    output: &mut BufReader<tokio::process::ChildStdout>,
    request: Envelope,
) -> Result<Envelope> {
    let expected_id = request.request_id.clone();
    input.write_all(&encode_line(&request).map_err(protocol_error)?).await?;
    input.flush().await?;
    let line = read_bounded_line(output).await?;
    if line.is_empty() {
        return Err(AppError::Plugin(
            "Configured plugin returned an invalid response size".to_string(),
        ));
    }
    let response = decode_line(&line).map_err(protocol_error)?;
    if response.request_id != expected_id {
        return Err(AppError::Plugin(
            "Configured plugin response correlation failed".to_string(),
        ));
    }
    Ok(response)
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok(line);
        }
        let consumed = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        if line.len() + consumed > MAX_PROTOCOL_LINE_BYTES + 1 {
            return Err(AppError::Plugin(
                "Configured plugin returned an invalid response size".to_string(),
            ));
        }
        line.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);
        if line.last() == Some(&b'\n') {
            return Ok(line);
        }
    }
}

fn protocol_error(error: impl std::fmt::Display) -> AppError {
    AppError::Plugin(format!("Configured plugin protocol validation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capability::{CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityManifest, PluginId};
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
            let path = executable_path(&self.bin, executable_names(package).first().unwrap());
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
        let executable = executable_path(&manager.cargo.bin, "rho-plugin-fixture");
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
            },
        )
        .unwrap();
        manager.remove("fixture").unwrap();
        assert!(executable.exists());
        assert!(manager.cargo.events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn protocol_validation_reader_rejects_oversized_lines_before_parsing() {
        let bytes = vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 2];
        let mut reader = BufReader::new(bytes.as_slice());
        let error = read_bounded_line(&mut reader).await.unwrap_err().to_string();
        assert!(error.contains("invalid response size"));
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
                "#!/bin/sh\nread first\nprintf '%s\\n' '{{\"protocol_version\":1,\"request_id\":\"install-handshake\",\"type\":\"terminal_response\",\"result\":{{\"kind\":\"handshake\",\"response\":{{\"selected_version\":1}}}}}}'\nread second\nprintf '%s\\n' '{{\"protocol_version\":1,\"request_id\":\"install-discovery\",\"type\":\"terminal_response\",\"result\":{{\"kind\":\"discovery\",\"response\":{{\"manifest\":{manifest}}}}}}}'\n"
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
}
