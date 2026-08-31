use rho_core::workspace::Workspace;
use rho_sdk::contract::{NetworkAccess, OperationEffect, PathScope};
use serde_json::Value;

#[cfg(test)]
mod tests;

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
    allow_private_network: bool,
}

impl SafetyFloor {
    pub fn new(workspace: Workspace, allow_private_network: bool) -> Self {
        Self {
            workspace,
            allow_private_network,
        }
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
        rho_core::net::validate_url(raw_url, self.allow_private_network)
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
    rho_sdk::schema::CompiledSchema::compile(schema)
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
