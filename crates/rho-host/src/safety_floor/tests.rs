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
    let floor = SafetyFloor::new(Workspace::with_exclusions(&root, [&config, &sessions]), false);
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
