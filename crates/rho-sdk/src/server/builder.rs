use crate::capability::{
    CAPABILITY_API_VERSION, CapabilityDeclaration, CapabilityId, CapabilityManifest, CapabilityValidationError,
    PLUGIN_PROTOCOL_VERSION, PluginId,
};
use crate::contract::{
    CapabilityDescriptor, CommandCapability, ContextCapability, LifecycleCapability, PermissionCapability,
    ProviderCapability, SkillCapability, ToolCapability,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Plugin {
    pub manifest: CapabilityManifest,
    pub descriptors: Vec<CapabilityDescriptor>,
    pub providers: BTreeMap<CapabilityId, Arc<dyn ProviderCapability>>,
    pub tools: BTreeMap<CapabilityId, Arc<dyn ToolCapability>>,
    pub permissions: BTreeMap<CapabilityId, Arc<dyn PermissionCapability>>,
    pub commands: BTreeMap<CapabilityId, Arc<dyn CommandCapability>>,
    pub lifecycles: BTreeMap<CapabilityId, Arc<dyn LifecycleCapability>>,
    pub skills: BTreeMap<CapabilityId, Arc<dyn SkillCapability>>,
    pub contexts: BTreeMap<CapabilityId, Arc<dyn ContextCapability>>,
}

pub struct PluginBuilder {
    plugin_id: String,
    version: String,
    providers: Vec<Arc<dyn ProviderCapability>>,
    tools: Vec<(Arc<dyn ToolCapability>, Option<CapabilityId>)>,
    permissions: Vec<Arc<dyn PermissionCapability>>,
    commands: Vec<Arc<dyn CommandCapability>>,
    lifecycles: Vec<Arc<dyn LifecycleCapability>>,
    skills: Vec<Arc<dyn SkillCapability>>,
    contexts: Vec<Arc<dyn ContextCapability>>,
}

impl PluginBuilder {
    pub fn new(plugin_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            version: version.into(),
            providers: Vec::new(),
            tools: Vec::new(),
            permissions: Vec::new(),
            commands: Vec::new(),
            lifecycles: Vec::new(),
            skills: Vec::new(),
            contexts: Vec::new(),
        }
    }

    pub fn provider(mut self, provider: impl ProviderCapability + 'static) -> Self {
        self.providers.push(Arc::new(provider));
        self
    }

    pub fn tool(mut self, tool: impl ToolCapability + 'static) -> Self {
        self.tools.push((Arc::new(tool), None));
        self
    }

    pub fn tool_replacing(mut self, tool: impl ToolCapability + 'static, replaces: CapabilityId) -> Self {
        self.tools.push((Arc::new(tool), Some(replaces)));
        self
    }

    pub fn command(mut self, command: impl CommandCapability + 'static) -> Self {
        self.commands.push(Arc::new(command));
        self
    }

    pub fn context(mut self, context: impl ContextCapability + 'static) -> Self {
        self.contexts.push(Arc::new(context));
        self
    }

    pub fn lifecycle(mut self, lifecycle: impl LifecycleCapability + 'static) -> Self {
        self.lifecycles.push(Arc::new(lifecycle));
        self
    }

    pub fn permission(mut self, permission: impl PermissionCapability + 'static) -> Self {
        self.permissions.push(Arc::new(permission));
        self
    }

    pub fn skill(mut self, skill: impl SkillCapability + 'static) -> Self {
        self.skills.push(Arc::new(skill));
        self
    }

    pub fn build(self) -> Result<Plugin, CapabilityValidationError> {
        let plugin_id: PluginId = self.plugin_id.parse()?;
        let mut declarations = Vec::new();
        let mut descriptors = Vec::new();

        let mut providers = BTreeMap::new();
        for p in self.providers {
            let desc = p.descriptor();
            let id = desc.id.clone();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Provider(desc));
            providers.insert(id, p);
        }

        let mut tools = BTreeMap::new();
        for (t, replaces) in self.tools {
            let desc = t.descriptor();
            let id = desc.id.clone();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces,
            });
            descriptors.push(CapabilityDescriptor::Tool(desc));
            tools.insert(id, t);
        }

        let mut commands = BTreeMap::new();
        for c in self.commands {
            let desc = c.descriptor();
            let id = desc.id.clone();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Command(desc));
            commands.insert(id, c);
        }

        let mut contexts = BTreeMap::new();
        for ctx in self.contexts {
            let desc = ctx.descriptor();
            let id = desc.id.clone();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Context(desc));
            contexts.insert(id, ctx);
        }

        let mut lifecycles = BTreeMap::new();
        for l in self.lifecycles {
            let id = l.id();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Lifecycle { id: id.clone() });
            lifecycles.insert(id, l);
        }

        let mut permissions = BTreeMap::new();
        for perm in self.permissions {
            let id = perm.id();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Permission { id: id.clone() });
            permissions.insert(id, perm);
        }

        let mut skills = BTreeMap::new();
        for s in self.skills {
            let id = s.id();
            declarations.push(CapabilityDeclaration {
                id: id.clone(),
                replaces: None,
            });
            descriptors.push(CapabilityDescriptor::Skill { id: id.clone() });
            skills.insert(id, s);
        }

        let manifest = CapabilityManifest {
            plugin_id,
            plugin_version: self.version,
            api_version: CAPABILITY_API_VERSION,
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            capabilities: declarations,
        };

        Ok(Plugin {
            manifest,
            descriptors,
            providers,
            tools,
            permissions,
            commands,
            lifecycles,
            skills,
            contexts,
        })
    }
}
