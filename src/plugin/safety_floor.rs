use crate::plugin::contract::{NetworkAccess, OperationEffect, PathScope};
use crate::tools::web::HttpClient;
use crate::tools::workspace::Workspace;
use serde_json::Value;

const PROTECTED_READ_MESSAGE: &str = "reading protected rho configuration or session storage is not permitted";
const ESCAPED_READ_MESSAGE: &str = "read target is outside the permitted workspace";
const DENIED_WRITE_MESSAGE: &str = "write target is outside the permitted workspace or is protected";
const EXPLICIT_HOSTS_MESSAGE: &str = "explicit-host network access requires a host allowlist";
const DENIED_NETWORK_MESSAGE: &str = "network target is not permitted";
const INVALID_SCHEMA_MESSAGE: &str = "failed to parse tool arguments: arguments do not match the declared schema";

/// A host-floor rejection that no permission policy may overturn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FloorDenial {
    #[error("{0}")]
    InvalidArguments(String),
    #[error("{0}")]
    Operation(String),
}

/// Declared operation submitted to the host floor before any permission
/// evaluation or tool invocation, for built-in and external tools alike.
pub struct FloorRequest<'a> {
    pub schema: &'a Value,
    pub effects: &'a [OperationEffect],
    pub arguments: &'a Value,
}

/// Host-owned safety floor: argument schema, workspace containment, protected
/// `rho` configuration/session/credential locations, and network policy.
/// Denials are final; permission policies may only restrict further.
#[derive(Clone)]
pub struct SafetyFloor {
    workspace: Workspace,
    http: HttpClient,
}

impl SafetyFloor {
    pub fn new(workspace: Workspace, http: HttpClient) -> Self {
        Self { workspace, http }
    }

    pub fn enforce(&self, request: FloorRequest<'_>) -> Result<(), FloorDenial> {
        validate_schema(request.schema, request.arguments)?;
        for effect in request.effects {
            self.check_effect(effect, request.arguments)?;
        }
        Ok(())
    }

    fn check_effect(&self, effect: &OperationEffect, arguments: &Value) -> Result<(), FloorDenial> {
        match effect {
            OperationEffect::ReadPath { scope } => self.check_read(*scope, arguments),
            OperationEffect::WritePath { .. } => self.check_write(arguments),
            OperationEffect::Network { access } => self.check_network(*access, arguments),
            OperationEffect::ExecuteProcess | OperationEffect::UserInteraction => Ok(()),
        }
    }

    fn check_read(&self, scope: PathScope, arguments: &Value) -> Result<(), FloorDenial> {
        let path = required_path(arguments)?;
        if self.workspace.is_excluded(path) {
            return Err(FloorDenial::Operation(PROTECTED_READ_MESSAGE.to_string()));
        }
        if scope == PathScope::Workspace {
            self.require_within(path, ESCAPED_READ_MESSAGE)?;
        }
        Ok(())
    }

    fn check_write(&self, arguments: &Value) -> Result<(), FloorDenial> {
        let path = required_path(arguments)?;
        if self.workspace.can_mutate(path) {
            Ok(())
        } else {
            Err(FloorDenial::Operation(DENIED_WRITE_MESSAGE.to_string()))
        }
    }

    fn check_network(&self, access: NetworkAccess, arguments: &Value) -> Result<(), FloorDenial> {
        match access {
            NetworkAccess::None => Ok(()),
            NetworkAccess::ExplicitHosts => Err(FloorDenial::Operation(EXPLICIT_HOSTS_MESSAGE.to_string())),
            NetworkAccess::PublicInternet => match arguments.get("url").and_then(Value::as_str) {
                Some(url) => self.check_url(url),
                None => Ok(()),
            },
        }
    }

    fn check_url(&self, raw_url: &str) -> Result<(), FloorDenial> {
        self.http
            .validate_url(raw_url)
            .map(|_| ())
            .map_err(|_| FloorDenial::Operation(DENIED_NETWORK_MESSAGE.to_string()))
    }

    fn require_within(&self, path: &str, message: &str) -> Result<(), FloorDenial> {
        if self.workspace.is_within(path) {
            Ok(())
        } else {
            Err(FloorDenial::Operation(message.to_string()))
        }
    }
}

fn validate_schema(schema: &Value, arguments: &Value) -> Result<(), FloorDenial> {
    crate::plugin::schema::CompiledSchema::compile(schema)
        .and_then(|schema| schema.validate(arguments))
        .map_err(|_| FloorDenial::InvalidArguments(INVALID_SCHEMA_MESSAGE.to_string()))
}

fn required_path(arguments: &Value) -> Result<&str, FloorDenial> {
    arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| FloorDenial::InvalidArguments("path must be a non-empty string".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    const WRITE_EFFECTS: &[OperationEffect] = &[OperationEffect::WritePath {
        scope: PathScope::Workspace,
    }];
    const READ_EFFECTS: &[OperationEffect] = &[OperationEffect::ReadPath {
        scope: PathScope::Explicit,
    }];
    const NETWORK_EFFECTS: &[OperationEffect] = &[OperationEffect::Network {
        access: NetworkAccess::PublicInternet,
    }];
    fn object_schema() -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}, "url": {"type": "string"}}
        })
    }

    struct FloorFixture {
        root: PathBuf,
        floor: SafetyFloor,
    }

    impl Drop for FloorFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> FloorFixture {
        let root = std::env::temp_dir().join(format!("safety_floor_{}", uuid::Uuid::new_v4()));
        let config = root.join("rho");
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&sessions).unwrap();
        let floor = SafetyFloor::new(
            Workspace::with_exclusions(&root, [&config, &sessions]),
            HttpClient::new(false).unwrap(),
        );
        FloorFixture { root, floor }
    }

    fn enforce(fixture: &FloorFixture, effects: &[OperationEffect], arguments: &Value) -> Result<(), FloorDenial> {
        fixture.floor.enforce(FloorRequest {
            schema: &object_schema(),
            effects,
            arguments,
        })
    }

    #[test]
    fn malformed_arguments_fail_schema_validation() {
        let fixture = fixture();
        let denial = enforce(&fixture, WRITE_EFFECTS, &json!({})).unwrap_err();
        assert!(matches!(denial, FloorDenial::InvalidArguments(_)));
        let malformed = enforce(&fixture, NETWORK_EFFECTS, &json!({"url": 3})).unwrap_err();
        assert!(matches!(malformed, FloorDenial::InvalidArguments(_)));
    }

    #[test]
    fn protected_config_session_and_credential_locations_are_denied() {
        let fixture = fixture();
        let config = fixture.root.join("rho");
        let denied = [
            json!({"path": config.join("config.toml").display().to_string()}),
            json!({"path": config.join("credentials.json").display().to_string()}),
            json!({"path": fixture.root.join("sessions").join("run.jsonl").display().to_string()}),
        ];
        for arguments in denied {
            let denial = enforce(&fixture, WRITE_EFFECTS, &arguments).unwrap_err();
            assert!(matches!(denial, FloorDenial::Operation(_)), "{arguments}");
            assert!(enforce(&fixture, READ_EFFECTS, &arguments).is_err(), "{arguments}");
        }
    }

    #[test]
    fn git_metadata_writes_and_workspace_escapes_are_denied() {
        let fixture = fixture();
        std::fs::create_dir_all(fixture.root.join(".git")).unwrap();
        let denied = [
            (WRITE_EFFECTS, json!({"path": ".git/config"})),
            (WRITE_EFFECTS, json!({"path": "../outside.txt"})),
        ];
        for (effects, arguments) in denied {
            assert!(
                matches!(enforce(&fixture, effects, &arguments), Err(FloorDenial::Operation(_))),
                "{arguments}"
            );
        }
        assert!(enforce(&fixture, WRITE_EFFECTS, &json!({"path": "out.txt"})).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_denied() {
        let outside = std::env::temp_dir().join(format!("safety_floor_out_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&outside).unwrap();
        let fixture = fixture();
        std::os::unix::fs::symlink(&outside, fixture.root.join("escape")).unwrap();
        let arguments = json!({"path": "escape/out.txt"});

        assert!(matches!(
            enforce(&fixture, WRITE_EFFECTS, &arguments),
            Err(FloorDenial::Operation(_))
        ));
    }

    #[test]
    fn private_network_targets_are_denied_and_public_targets_pass() {
        let fixture = fixture();
        assert!(matches!(
            enforce(
                &fixture,
                NETWORK_EFFECTS,
                &json!({"path": "report.txt", "url": "http://127.0.0.1/private"})
            ),
            Err(FloorDenial::Operation(_))
        ));
        assert!(
            enforce(
                &fixture,
                NETWORK_EFFECTS,
                &json!({"path": "report.txt", "url": "https://example.com"})
            )
            .is_ok()
        );
    }

    #[test]
    fn explicit_host_network_access_requires_an_allowlist() {
        let fixture = fixture();
        let effects = [OperationEffect::Network {
            access: NetworkAccess::ExplicitHosts,
        }];
        assert!(matches!(
            enforce(
                &fixture,
                &effects,
                &json!({"path": "report.txt", "url": "https://example.com"})
            ),
            Err(FloorDenial::Operation(_))
        ));
    }
}
