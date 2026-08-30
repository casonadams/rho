use crate::resolver::CapabilityPlugin;
use rho_core::provider::ProviderId;
use rho_plugin_builtin::BuiltinToolCatalog;
use rho_sdk::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, PLUGIN_PROTOCOL_VERSION,
};

pub fn capability_plugin() -> CapabilityPlugin {
    let mut capabilities: Vec<_> = ProviderId::ALL
        .into_iter()
        .map(|provider| declaration("provider", provider.as_str()))
        .chain(
            BuiltinToolCatalog::descriptors()
                .iter()
                .map(|tool| declaration("tool", tool.id.name())),
        )
        .collect();
    capabilities.push(declaration("permission", "default"));
    CapabilityPlugin {
        manifest: CapabilityManifest {
            plugin_id: "rho.builtin".parse().unwrap(),
            plugin_version: env!("CARGO_PKG_VERSION").to_string(),
            api_version: CAPABILITY_API_VERSION,
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            capabilities,
        }
        .validate()
        .unwrap(),
        origin: rho_sdk::capability::PluginOrigin::BuiltIn,
        authorized_replacements: Default::default(),
        configured: false,
    }
}

fn declaration(kind: &str, name: &str) -> CapabilityDeclaration {
    CapabilityDeclaration {
        id: format!("{kind}:{name}").parse::<CapabilityId>().unwrap(),
        replaces: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_exposes_every_current_provider_tool_and_default_policy() {
        let plugin = capability_plugin();
        for id in [
            "provider:anthropic",
            "provider:openai",
            "provider:chatgpt",
            "provider:copilot",
            "tool:read",
            "tool:bash",
            "tool:ask_user_question",
            "permission:default",
        ] {
            assert!(
                plugin
                    .manifest
                    .capabilities
                    .iter()
                    .any(|item| item.id.to_string() == id)
            );
        }
        assert_eq!(
            plugin.manifest.capabilities.len(),
            ProviderId::ALL.len() + BuiltinToolCatalog::descriptors().len() + 1
        );
    }
}
