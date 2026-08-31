pub mod types;

pub use types::{
    ActiveCapability, CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityError, CapabilityId, CapabilityKind,
    CapabilityManifest, CapabilityValidationError, PLUGIN_PROTOCOL_VERSION, PluginId, PluginOrigin, ValidatedManifest,
};

#[cfg(test)]
mod tests;
