use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub const CAPABILITY_API_VERSION: u32 = 1;
pub const PLUGIN_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Provider,
    Tool,
    Permission,
    Command,
    Lifecycle,
    Skill,
}

impl Display for CapabilityKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Provider => "provider",
            Self::Tool => "tool",
            Self::Permission => "permission",
            Self::Command => "command",
            Self::Lifecycle => "lifecycle",
            Self::Skill => "skill",
        })
    }
}

impl FromStr for CapabilityKind {
    type Err = CapabilityValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "provider" => Ok(Self::Provider),
            "tool" => Ok(Self::Tool),
            "permission" => Ok(Self::Permission),
            "command" => Ok(Self::Command),
            "lifecycle" => Ok(Self::Lifecycle),
            "skill" => Ok(Self::Skill),
            _ => Err(CapabilityValidationError::InvalidCapabilityId(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId {
    kind: CapabilityKind,
    name: String,
}

impl CapabilityId {
    pub fn new(kind: CapabilityKind, name: impl Into<String>) -> Result<Self, CapabilityValidationError> {
        let name = name.into();
        if !is_valid_identifier(&name) {
            return Err(CapabilityValidationError::InvalidCapabilityId(format!("{kind}:{name}")));
        }
        Ok(Self { kind, name })
    }

    pub const fn kind(&self) -> CapabilityKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn require_kind(&self, expected: CapabilityKind) -> Result<(), CapabilityValidationError> {
        if self.kind == expected {
            Ok(())
        } else {
            Err(CapabilityValidationError::UnexpectedCapabilityKind {
                id: self.clone(),
                expected,
            })
        }
    }
}

impl Display for CapabilityId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.kind, self.name)
    }
}

impl FromStr for CapabilityId {
    type Err = CapabilityValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((kind, name)) = value.split_once(':') else {
            return Err(CapabilityValidationError::InvalidCapabilityId(value.to_string()));
        };
        if name.contains(':') {
            return Err(CapabilityValidationError::InvalidCapabilityId(value.to_string()));
        }
        Self::new(kind.parse()?, name)
    }
}

impl Serialize for CapabilityId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CapabilityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(value: impl Into<String>) -> Result<Self, CapabilityValidationError> {
        let value = value.into();
        if !is_valid_identifier(&value) {
            return Err(CapabilityValidationError::InvalidPluginId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PluginId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PluginId {
    type Err = CapabilityValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for PluginId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

fn is_valid_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 || !value.is_ascii() {
        return false;
    }
    let bytes = value.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginOrigin {
    BuiltIn,
    Configured {
        executable: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        package: Option<String>,
    },
}

impl PluginOrigin {
    pub fn validate(&self) -> Result<(), CapabilityValidationError> {
        match self {
            Self::BuiltIn => Ok(()),
            Self::Configured { executable, package }
                if executable.trim().is_empty() || package.as_ref().is_some_and(|value| value.trim().is_empty()) =>
            {
                Err(CapabilityValidationError::InvalidOrigin)
            }
            Self::Configured { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDeclaration {
    pub id: CapabilityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<CapabilityId>,
}

impl CapabilityDeclaration {
    pub fn validate(&self) -> Result<(), CapabilityValidationError> {
        if let Some(target) = &self.replaces
            && self.id.kind() != target.kind()
        {
            return Err(CapabilityValidationError::ReplacementKindMismatch {
                capability: self.id.clone(),
                target: target.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub plugin_id: PluginId,
    pub plugin_version: String,
    pub api_version: u32,
    pub protocol_version: u32,
    pub capabilities: Vec<CapabilityDeclaration>,
}

impl CapabilityManifest {
    pub fn validate(&self) -> Result<ValidatedManifest, CapabilityValidationError> {
        if self.api_version != CAPABILITY_API_VERSION {
            return Err(CapabilityValidationError::UnsupportedApiVersion(self.api_version));
        }
        if self.protocol_version != PLUGIN_PROTOCOL_VERSION {
            return Err(CapabilityValidationError::UnsupportedProtocolVersion(
                self.protocol_version,
            ));
        }
        if self.plugin_version.is_empty() || self.plugin_version.len() > 128 || !self.plugin_version.is_ascii() {
            return Err(CapabilityValidationError::InvalidPluginVersion);
        }

        let mut capabilities = self.capabilities.clone();
        capabilities.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seen = BTreeSet::new();
        for declaration in &capabilities {
            declaration.validate()?;
            if !seen.insert(declaration.id.clone()) {
                return Err(CapabilityValidationError::DuplicateCapability(declaration.id.clone()));
            }
        }

        Ok(ValidatedManifest {
            plugin_id: self.plugin_id.clone(),
            plugin_version: self.plugin_version.clone(),
            api_version: self.api_version,
            protocol_version: self.protocol_version,
            capabilities,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedManifest {
    pub plugin_id: PluginId,
    pub plugin_version: String,
    pub api_version: u32,
    pub protocol_version: u32,
    pub capabilities: Vec<CapabilityDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveCapability {
    pub id: CapabilityId,
    pub plugin_id: PluginId,
    pub origin: PluginOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<CapabilityId>,
}

impl ActiveCapability {
    pub fn validate(&self) -> Result<(), CapabilityValidationError> {
        self.origin.validate()?;
        CapabilityDeclaration {
            id: self.id.clone(),
            replaces: self.replaces.clone(),
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum CapabilityValidationError {
    #[error("invalid capability identifier: {0}")]
    InvalidCapabilityId(String),
    #[error("invalid plugin identifier: {0}")]
    InvalidPluginId(String),
    #[error("capability {id} does not have expected kind {expected}")]
    UnexpectedCapabilityKind { id: CapabilityId, expected: CapabilityKind },
    #[error("plugin origin is invalid")]
    InvalidOrigin,
    #[error("plugin version is invalid")]
    InvalidPluginVersion,
    #[error("duplicate capability declaration: {0}")]
    DuplicateCapability(CapabilityId),
    #[error("capability {capability} cannot replace different-kind target {target}")]
    ReplacementKindMismatch {
        capability: CapabilityId,
        target: CapabilityId,
    },
    #[error("unsupported capability API version: {0}")]
    UnsupportedApiVersion(u32),
    #[error("unsupported plugin protocol version: {0}")]
    UnsupportedProtocolVersion(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CapabilityError {
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
    #[error("permission denied: {message}")]
    PermissionDenied { message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("capability unavailable: {message}")]
    Unavailable { message: String },
    #[error("capability failed: {message}")]
    Failed { message: String },
}

#[cfg(test)]
mod tests {
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
        for valid in ["tool:bash", "provider:openai-compatible", "permission:org.default"] {
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
}
