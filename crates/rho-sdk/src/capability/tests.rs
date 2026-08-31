use super::*;

fn id(value: &str) -> CapabilityId {
    value.parse().unwrap()
}

fn manifest(capabilities: Vec<CapabilityDeclaration>) -> CapabilityManifest {
    CapabilityManifest {
        plugin_id: "fixture.plugin".parse().unwrap(),
        plugin_version: "1.0.0".to_string(),
        api_version: CAPABILITY_API_VERSION,
        protocol_version: PLUGIN_PROTOCOL_VERSION,
        capabilities,
    }
}

#[test]
fn validates_capability_ids() {
    for valid in [
        "tool:bash",
        "provider:openai-compatible",
        "permission:org.default",
        "context:kiln",
    ] {
        assert_eq!(valid.parse::<CapabilityId>().unwrap().to_string(), valid);
    }
    for invalid in ["bash", "tool:", "Tool:bash", "tool:Bash", "tool:-bash", "tool:bash:"] {
        assert!(invalid.parse::<CapabilityId>().is_err(), "{invalid}");
    }
}

#[test]
fn rejects_duplicate_and_cross_kind_replacements() {
    let duplicate = CapabilityDeclaration {
        id: id("tool:bash"),
        replaces: None,
    };
    assert!(matches!(
        manifest(vec![duplicate.clone(), duplicate]).validate(),
        Err(CapabilityValidationError::DuplicateCapability(_))
    ));

    let mismatch = CapabilityDeclaration {
        id: id("tool:shell"),
        replaces: Some(id("provider:openai")),
    };
    assert!(matches!(
        manifest(vec![mismatch]).validate(),
        Err(CapabilityValidationError::ReplacementKindMismatch { .. })
    ));
}

#[test]
fn rejects_unsupported_versions() {
    let mut value = manifest(Vec::new());
    value.api_version = 2;
    assert!(matches!(
        value.validate(),
        Err(CapabilityValidationError::UnsupportedApiVersion(2))
    ));
    value.api_version = 1;
    value.protocol_version = 2;
    assert!(matches!(
        value.validate(),
        Err(CapabilityValidationError::UnsupportedProtocolVersion(2))
    ));
}

#[test]
fn origins_and_active_metadata_validate() {
    let active = ActiveCapability {
        id: id("tool:bash"),
        plugin_id: "fixture.plugin".parse().unwrap(),
        origin: PluginOrigin::Configured {
            executable: "/plugins/bash".to_string(),
            package: None,
        },
        replaces: Some(id("tool:bash")),
    };
    active.validate().unwrap();
    assert_eq!(
        serde_json::to_string(&active.origin).unwrap(),
        r#"{"type":"configured","executable":"/plugins/bash"}"#
    );

    let invalid = PluginOrigin::Configured {
        executable: String::new(),
        package: None,
    };
    assert_eq!(invalid.validate(), Err(CapabilityValidationError::InvalidOrigin));
}

#[test]
fn validated_manifest_order_is_deterministic() {
    let validated = manifest(vec![
        CapabilityDeclaration {
            id: id("tool:write"),
            replaces: None,
        },
        CapabilityDeclaration {
            id: id("provider:openai"),
            replaces: None,
        },
        CapabilityDeclaration {
            id: id("tool:read"),
            replaces: None,
        },
    ])
    .validate()
    .unwrap();
    let ids: Vec<String> = validated.capabilities.iter().map(|item| item.id.to_string()).collect();
    assert_eq!(ids, ["provider:openai", "tool:read", "tool:write"]);
    assert_eq!(
        serde_json::to_string(&validated).unwrap(),
        serde_json::to_string(&validated).unwrap()
    );
}
