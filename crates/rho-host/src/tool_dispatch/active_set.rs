use super::neutral::NeutralActiveToolExecutor;
use super::types::{ActiveTool, DispatchContext};
use crate::external::ExternalPlugin;
use crate::loader::{ConfiguredStatus, PluginLoader};
use crate::permission::{PolicyEvaluator, PolicyFailureMode, PolicyLimits};
use crate::process::ProcessLimits;
use crate::resolver::{CapabilityPlugin, CapabilityResolver};
use crate::safety_floor::SafetyFloor;
use rho_core::config::Config;
use rho_core::error::Result;
use rho_plugin_builtin::BuiltinToolCatalog;
use rho_sdk::capability::{CapabilityId, CapabilityKind, PluginId, PluginOrigin};
use rho_sdk::contract::{
    CommandCapability, ContextCapability, ExecutionMode, LifecycleCapability, PermissionCapability, ToolCapability,
    ToolDescriptor,
};
use rig::tool::{DynamicTool, ToolContext};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct ActiveToolSet {
    pub(crate) tools: BTreeMap<String, ActiveTool>,
    pub(crate) contexts: BTreeMap<CapabilityId, Arc<dyn ContextCapability>>,
    pub(crate) commands: BTreeMap<String, Arc<dyn CommandCapability>>,
    pub(crate) lifecycles: Vec<Arc<dyn LifecycleCapability>>,
    pub(crate) floor: Arc<SafetyFloor>,
    pub(crate) policies: Arc<PolicyEvaluator>,
}

impl ActiveToolSet {
    pub fn builtins(config: &Config, base_dir: &Path) -> Result<Self> {
        let catalog = BuiltinToolCatalog::new(base_dir, config)?;
        let capabilities = catalog.into_capabilities();
        let tools = capabilities
            .into_iter()
            .map(|(id, capability)| {
                let descriptor = capability.descriptor();
                (
                    id.name().to_string(),
                    ActiveTool {
                        target_id: id,
                        descriptor,
                        capability,
                    },
                )
            })
            .collect();
        let fl = floor(config, base_dir)?;
        let pe = PolicyEvaluator::spawn(Vec::new(), PolicyFailureMode::Deny, PolicyLimits::default());
        Ok(Self {
            tools,
            contexts: BTreeMap::new(),
            commands: BTreeMap::new(),
            lifecycles: Vec::new(),
            floor: Arc::new(fl),
            policies: Arc::new(pe),
        })
    }

    pub async fn load(config: &Config, base_dir: &Path) -> Result<Self> {
        Self::load_with_executor(config, base_dir, None).await
    }

    pub async fn load_with_executor(
        config: &Config,
        base_dir: &Path,
        executor: Option<Arc<dyn rho_plugin_builtin::subagents::SubagentExecutor>>,
    ) -> Result<Self> {
        let builtins = BuiltinToolCatalog::new(base_dir, config)?.into_capabilities();
        let mut external_plugins = BTreeMap::<PluginId, ExternalPlugin>::new();
        let mut external_manifests = Vec::new();
        for candidate in PluginLoader::configured_candidates(&config.config_dir, &config.plugins) {
            if candidate.status != ConfiguredStatus::Eligible {
                continue;
            }
            let Ok(plugin) = ExternalPlugin::load(&candidate.path, ProcessLimits::default()).await else {
                continue;
            };
            if plugin.manifest().plugin_id.as_str() != candidate.name {
                continue;
            }
            let manifest = plugin.resolvable_manifest();
            let plugin_id = manifest.plugin_id.clone();
            external_manifests.push(CapabilityPlugin {
                manifest,
                origin: PluginOrigin::Configured {
                    executable: candidate.path.display().to_string(),
                    package: candidate.package,
                },
                authorized_replacements: candidate.replaces,
                configured: true,
            });
            external_plugins.insert(plugin_id, plugin);
        }

        let resolution = CapabilityResolver::resolve(vec![crate::builtin::capability_plugin()], external_manifests);
        let mut tools = BTreeMap::new();
        let mut contexts = BTreeMap::new();
        let mut commands = BTreeMap::new();
        let mut lifecycles = Vec::new();
        let mut policies: Vec<Arc<dyn PermissionCapability>> = Vec::new();
        for (target_id, active) in resolution.active {
            if target_id.kind() == CapabilityKind::Permission {
                if active.plugin_id.as_str() == "rho.builtin" {
                    continue;
                }
                if let Some(plugin) = external_plugins.get(&active.plugin_id)
                    && let Ok(policy) = plugin.permission(&active.id)
                {
                    policies.push(Arc::new(policy) as Arc<dyn PermissionCapability>);
                }
                continue;
            }
            if target_id.kind() == CapabilityKind::Context {
                if let Some(plugin) = external_plugins.get(&active.plugin_id)
                    && let Ok(ctx_cap) = plugin.context(&active.id)
                {
                    contexts.insert(target_id.clone(), Arc::new(ctx_cap) as Arc<dyn ContextCapability>);
                }
                continue;
            }
            if target_id.kind() == CapabilityKind::Command {
                if let Some(plugin) = external_plugins.get(&active.plugin_id)
                    && let Ok(cmd_cap) = plugin.command(&active.id)
                {
                    commands.insert(
                        cmd_cap.descriptor().name.clone(),
                        Arc::new(cmd_cap) as Arc<dyn CommandCapability>,
                    );
                }
                continue;
            }
            if target_id.kind() == CapabilityKind::Lifecycle {
                if let Some(plugin) = external_plugins.get(&active.plugin_id)
                    && let Ok(lifecycle_cap) = plugin.lifecycle(&active.id)
                {
                    lifecycles.push(Arc::new(lifecycle_cap) as Arc<dyn LifecycleCapability>);
                }
                continue;
            }
            if target_id.kind() != CapabilityKind::Tool {
                continue;
            }
            let capability: Arc<dyn ToolCapability> = if active.plugin_id.as_str() == "rho.builtin" {
                let Some(capability) = builtins.get(&active.id) else {
                    continue;
                };
                Arc::clone(capability)
            } else {
                let Some(plugin) = external_plugins.get(&active.plugin_id) else {
                    continue;
                };
                let Ok(capability) = plugin.tool(&active.id) else {
                    continue;
                };
                Arc::new(capability)
            };
            tools.insert(
                target_id.name().to_string(),
                ActiveTool {
                    target_id,
                    descriptor: capability.descriptor(),
                    capability,
                },
            );
        }

        let mcp_capabilities = rho_plugin_builtin::mcp::load_mcp_capabilities(config, base_dir).await;
        for (target_id, capability) in mcp_capabilities {
            let descriptor = capability.descriptor();
            tools.insert(
                target_id.name().to_string(),
                ActiveTool {
                    target_id,
                    descriptor,
                    capability,
                },
            );
        }

        let subagent_capabilities =
            rho_plugin_builtin::subagents::load_subagent_capabilities(config, base_dir, executor);
        for (target_id, capability) in subagent_capabilities {
            let descriptor = capability.descriptor();
            tools.insert(
                target_id.name().to_string(),
                ActiveTool {
                    target_id,
                    descriptor,
                    capability,
                },
            );
        }

        Ok(Self {
            tools,
            contexts,
            commands,
            lifecycles,
            floor: Arc::new(floor(config, base_dir)?),
            policies: Arc::new(PolicyEvaluator::spawn(
                policies,
                PolicyFailureMode::Deny,
                PolicyLimits::default(),
            )),
        })
    }

    pub fn active_contexts(&self) -> Vec<Arc<dyn ContextCapability>> {
        self.contexts.values().cloned().collect()
    }

    pub fn active_commands(&self) -> BTreeMap<String, Arc<dyn CommandCapability>> {
        self.commands.clone()
    }

    pub fn active_lifecycles(&self) -> Vec<Arc<dyn LifecycleCapability>> {
        self.lifecycles.clone()
    }

    pub fn definitions(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|tool| tool.descriptor.clone()).collect()
    }

    pub fn provider_definitions(&self) -> Vec<rho_sdk::contract::ProviderToolDefinition> {
        self.tools
            .iter()
            .map(|(name, tool)| rho_sdk::contract::ProviderToolDefinition {
                id: format!("tool:{name}").parse().unwrap(),
                description: tool.descriptor.description.clone(),
                argument_schema: tool.descriptor.argument_schema.clone(),
            })
            .collect()
    }

    pub fn execution_mode(&self, tool_name: &str) -> ExecutionMode {
        self.tools
            .get(tool_name)
            .map(|tool| tool.descriptor.execution_mode)
            .unwrap_or(ExecutionMode::Sequential)
    }

    pub fn neutral_executor(self: &Arc<Self>, context: ToolContext) -> NeutralActiveToolExecutor {
        NeutralActiveToolExecutor::new(Arc::clone(self), context)
    }

    pub fn into_rig_tools(self) -> Vec<DynamicTool> {
        self.tools
            .into_iter()
            .map(|(name, tool)| {
                let description = tool.descriptor.description.clone();
                let mut schema = tool.descriptor.argument_schema.clone();
                rho_plugin_builtin::tools::normalize_schema(&mut schema);
                let floor = Arc::clone(&self.floor);
                let policies = Arc::clone(&self.policies);
                DynamicTool::new(name, description, schema, move |context, arguments| {
                    let floor = Arc::clone(&floor);
                    let policies = Arc::clone(&policies);
                    let tool = tool.clone();
                    Box::pin(async move {
                        tool.dispatch(
                            DispatchContext {
                                floor: &floor,
                                policies: &policies,
                                tool: context,
                            },
                            arguments,
                        )
                        .await
                    })
                })
            })
            .collect()
    }
}

pub(crate) fn floor(config: &Config, base_dir: &Path) -> Result<SafetyFloor> {
    let workspace = rho_core::workspace::Workspace::with_exclusions(
        base_dir,
        [config.config_dir.clone(), config.sessions_dir.clone()],
    );
    Ok(SafetyFloor::new(workspace, config.allow_private_network))
}
