use crate::config::Config;
use crate::error::Result;
use crate::plugin::builtin_tools::BuiltinToolCatalog;
use crate::plugin::capability::{CapabilityError, CapabilityId, CapabilityKind, PluginId, PluginOrigin};
use crate::plugin::contract::{
    InteractionRequest, InteractionResponse, NetworkAccess, OperationEffect, PathScope, ToolCapability, ToolDescriptor,
    ToolHost, ToolInvocationRequest,
};
use crate::plugin::external::ExternalPlugin;
use crate::plugin::loader::{ConfiguredStatus, PluginLoader};
use crate::plugin::process::ProcessLimits;
use crate::plugin::resolver::{CapabilityPlugin, CapabilityResolver};
use crate::tools::approval::enforce_approval;
use crate::tools::ask_user::{QuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
use crate::tools::web::HttpClient;
use crate::tools::workspace::Workspace;
use async_trait::async_trait;
use rig::tool::{DynamicTool, ToolContext, ToolExecutionError, ToolOutput};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
struct ActiveTool {
    target_id: CapabilityId,
    descriptor: ToolDescriptor,
    capability: Arc<dyn ToolCapability>,
}

#[derive(Clone)]
struct HostFloor {
    base_dir: PathBuf,
    excluded: Vec<PathBuf>,
    http: HttpClient,
}

struct DispatchContext<'a> {
    floor: &'a HostFloor,
    tool: &'a mut ToolContext,
}

#[derive(Clone)]
pub struct ActiveToolSet {
    tools: BTreeMap<String, ActiveTool>,
    floor: Arc<HostFloor>,
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
        Ok(Self {
            tools,
            floor: floor(config, base_dir)?,
        })
    }

    pub async fn load(config: &Config, base_dir: &Path) -> Result<Self> {
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

        let resolution =
            CapabilityResolver::resolve(vec![crate::plugin::builtin::capability_plugin()], external_manifests);
        let mut tools = BTreeMap::new();
        for (target_id, active) in resolution.active {
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
        Ok(Self {
            tools,
            floor: floor(config, base_dir)?,
        })
    }

    pub fn definitions(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|tool| tool.descriptor.clone()).collect()
    }

    pub fn provider_definitions(&self) -> Vec<crate::plugin::contract::ProviderToolDefinition> {
        self.tools
            .iter()
            .map(|(name, tool)| crate::plugin::contract::ProviderToolDefinition {
                id: format!("tool:{name}").parse().unwrap(),
                description: tool.descriptor.description.clone(),
                argument_schema: tool.descriptor.argument_schema.clone(),
            })
            .collect()
    }

    pub fn neutral_executor(self: &Arc<Self>, context: ToolContext) -> NeutralActiveToolExecutor {
        NeutralActiveToolExecutor {
            tools: Arc::clone(self),
            context: tokio::sync::Mutex::new(context),
        }
    }

    pub fn into_rig_tools(self) -> Vec<DynamicTool> {
        self.tools
            .into_iter()
            .map(|(name, tool)| {
                let description = tool.descriptor.description.clone();
                let schema = tool.descriptor.argument_schema.clone();
                let floor = Arc::clone(&self.floor);
                DynamicTool::new(name, description, schema, move |context, arguments| {
                    let floor = Arc::clone(&floor);
                    let tool = tool.clone();
                    Box::pin(async move {
                        tool.dispatch(
                            DispatchContext {
                                floor: &floor,
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

fn floor(config: &Config, base_dir: &Path) -> Result<Arc<HostFloor>> {
    Ok(Arc::new(HostFloor {
        base_dir: base_dir.to_path_buf(),
        excluded: vec![config.config_dir.clone(), config.sessions_dir.clone()],
        http: HttpClient::new(config.allow_private_network)?,
    }))
}

impl ActiveTool {
    async fn dispatch(
        &self,
        runtime: DispatchContext<'_>,
        arguments: serde_json::Value,
    ) -> std::result::Result<ToolOutput, ToolExecutionError> {
        crate::plugin::schema::CompiledSchema::compile(&self.descriptor.argument_schema)
            .and_then(|schema| schema.validate(&arguments))
            .map_err(|_| {
                ToolExecutionError::invalid_args(
                    "failed to parse tool arguments: arguments do not match the declared schema",
                )
            })?;
        self.validate_host_floor(runtime.floor, &arguments)?;
        enforce_approval(runtime.tool, self.target_id.name(), &arguments)?;
        let invocation_context = runtime
            .tool
            .get::<crate::plugin::contract::InvocationContext>()
            .cloned()
            .unwrap_or_else(|| crate::plugin::contract::InvocationContext {
                session_id: String::new(),
                working_directory: runtime.floor.base_dir.display().to_string(),
                has_interactive_ui: runtime.tool.get::<QuestionPort>().is_some(),
            });
        let host = RigToolHost(runtime.tool);
        let response = self
            .capability
            .invoke(
                &host,
                ToolInvocationRequest {
                    arguments,
                    context: invocation_context,
                },
            )
            .await
            .map_err(map_capability_error)?;
        if response.is_error {
            Err(ToolExecutionError::other(response.content))
        } else {
            Ok(ToolOutput::text(response.content))
        }
    }

    fn validate_host_floor(
        &self,
        floor: &HostFloor,
        arguments: &serde_json::Value,
    ) -> std::result::Result<(), ToolExecutionError> {
        let workspace = Workspace::with_exclusions(&floor.base_dir, &floor.excluded);
        let mut effects = self
            .descriptor
            .effects
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(declaration) = crate::plugin::builtin_tools::DECLARATIONS
            .iter()
            .find(|declaration| declaration.name == self.target_id.name())
        {
            effects.extend(declaration.descriptor().effects);
        }
        for effect in &effects {
            match effect {
                OperationEffect::ReadPath { scope } => {
                    let path = required_string(arguments, "path")?;
                    if workspace.is_excluded(path) {
                        return Err(ToolExecutionError::permission_denied(
                            "reading protected rho configuration or session storage is not permitted",
                        ));
                    }
                    if *scope == PathScope::Workspace && !workspace.is_within(path) {
                        return Err(ToolExecutionError::permission_denied(
                            "read target is outside the permitted workspace",
                        ));
                    }
                }
                OperationEffect::WritePath { .. } => {
                    let path = required_string(arguments, "path")?;
                    if !workspace.can_mutate(path) {
                        return Err(ToolExecutionError::permission_denied(
                            "write target is outside the permitted workspace or is protected",
                        ));
                    }
                }
                OperationEffect::Network {
                    access: NetworkAccess::ExplicitHosts,
                } => {
                    return Err(ToolExecutionError::permission_denied(
                        "explicit-host network access requires a host allowlist",
                    ));
                }
                OperationEffect::Network {
                    access: NetworkAccess::PublicInternet,
                } => {
                    if let Some(url) = arguments.get("url").and_then(serde_json::Value::as_str) {
                        floor
                            .http
                            .validate_url(url)
                            .map_err(|_| ToolExecutionError::permission_denied("network target is not permitted"))?;
                    }
                }
                OperationEffect::Network {
                    access: NetworkAccess::None,
                }
                | OperationEffect::ExecuteProcess
                | OperationEffect::UserInteraction => {}
            }
        }
        Ok(())
    }
}

fn required_string<'a>(
    arguments: &'a serde_json::Value,
    field: &str,
) -> std::result::Result<&'a str, ToolExecutionError> {
    arguments
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolExecutionError::invalid_args(format!("{field} must be a non-empty string")))
}

pub struct NeutralActiveToolExecutor {
    tools: Arc<ActiveToolSet>,
    context: tokio::sync::Mutex<ToolContext>,
}

#[async_trait]
impl crate::engine::provider::host_loop::NeutralToolExecutor for NeutralActiveToolExecutor {
    async fn execute(
        &self,
        call: crate::engine::provider::host_loop::NeutralToolCall,
    ) -> std::result::Result<
        crate::engine::provider::host_loop::NeutralToolResult,
        crate::engine::provider::host_loop::NeutralTurnError,
    > {
        let crate::engine::provider::host_loop::NeutralToolCall {
            call_id,
            tool_id,
            arguments,
        } = call;
        let tool =
            self.tools.tools.get(tool_id.name()).ok_or_else(|| {
                crate::engine::provider::host_loop::NeutralTurnError::UnknownTool(tool_id.to_string())
            })?;
        let mut context = self.context.lock().await;
        let internal_call_id = uuid::Uuid::new_v4().to_string();
        let capability = context.get::<crate::tools::ApprovalCapability>().cloned();
        if let Some(capability) = &capability {
            crate::tools::authorize_dispatch(
                capability,
                crate::tools::DispatchedCall {
                    internal_call_id: &internal_call_id,
                    tool_name: tool_id.name(),
                    arguments: &arguments,
                },
            )
            .await;
        }
        let result = tool
            .dispatch(
                DispatchContext {
                    floor: &self.tools.floor,
                    tool: &mut context,
                },
                arguments.clone(),
            )
            .await;
        match result {
            Ok(output) => {
                if let Some(capability) = &capability {
                    crate::tools::approval::emit_tool_finished(
                        capability,
                        crate::tools::DispatchedCall {
                            internal_call_id: &internal_call_id,
                            tool_name: tool_id.name(),
                            arguments: &arguments,
                        },
                        crate::tools::DispatchedResult {
                            output: output.as_text().unwrap_or_default().to_string(),
                            status: "success",
                        },
                    );
                }
                Ok(crate::engine::provider::host_loop::NeutralToolResult {
                    content: output.as_text().unwrap_or_default().to_string(),
                    is_error: false,
                })
            }
            Err(error) => {
                if let Some(capability) = &capability {
                    let status = if error.is_refusal() { "denied" } else { "error" };
                    crate::tools::approval::emit_tool_finished(
                        capability,
                        crate::tools::DispatchedCall {
                            internal_call_id: &internal_call_id,
                            tool_name: tool_id.name(),
                            arguments: &arguments,
                        },
                        crate::tools::DispatchedResult {
                            output: error.model_output().as_text().unwrap_or_default().to_string(),
                            status,
                        },
                    );
                }
                Err(crate::engine::provider::host_loop::NeutralTurnError::Tool(format!(
                    "{call_id}: {}",
                    error.message()
                )))
            }
        }
    }
}

fn map_capability_error(error: CapabilityError) -> ToolExecutionError {
    match error {
        CapabilityError::InvalidRequest { message } => ToolExecutionError::invalid_args(message),
        CapabilityError::PermissionDenied { message } => ToolExecutionError::permission_denied(message),
        CapabilityError::Cancelled => ToolExecutionError::cancelled("tool operation cancelled"),
        CapabilityError::Unavailable { message } => ToolExecutionError::provider(message),
        CapabilityError::Failed { message } => ToolExecutionError::other(message),
    }
}

struct RigToolHost<'a>(&'a ToolContext);

#[async_trait]
impl ToolHost for RigToolHost<'_> {
    async fn interact(&self, request: InteractionRequest) -> std::result::Result<InteractionResponse, CapabilityError> {
        let port = self
            .0
            .get::<QuestionPort>()
            .ok_or_else(|| CapabilityError::Unavailable {
                message: "interactive question port is unavailable".to_string(),
            })?;
        let response = port
            .ask(UserQuestion {
                question: request.question,
                header: request.header,
                options: request
                    .options
                    .into_iter()
                    .map(|option| UserQuestionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
                allow_custom: request.allow_custom,
            })
            .await
            .map_err(|error| CapabilityError::Failed {
                message: error.to_string(),
            })?;
        Ok(match response {
            UserAnswer::Selected(index) => InteractionResponse::Selected(index),
            UserAnswer::Custom(value) => InteractionResponse::Custom(value),
            UserAnswer::Cancelled => InteractionResponse::Cancelled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::contract::{ToolDescriptor, ToolInvocationResponse};
    use crate::tools::approval::{
        ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, ToolEvent, approval_context,
    };
    use crate::tools::ask_user::{InteractiveQuestionPort, UserAnswer, UserQuestion};
    use rig::tool::{ToolErrorKind, ToolSet};

    struct ApprovalSink;

    #[async_trait]
    impl ApprovalEventSink for ApprovalSink {
        async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Approved
        }

        fn emit(&self, _event: ToolEvent) {}
    }

    struct AnswerPort;

    #[async_trait]
    impl InteractiveQuestionPort for AnswerPort {
        async fn ask(&self, _question: UserQuestion) -> std::result::Result<UserAnswer, crate::error::AppError> {
            Ok(UserAnswer::Custom("host answer".to_string()))
        }
    }

    struct FixtureTool(ToolDescriptor, Arc<std::sync::atomic::AtomicBool>);

    #[async_trait]
    impl ToolCapability for FixtureTool {
        fn descriptor(&self) -> ToolDescriptor {
            self.0.clone()
        }

        async fn invoke(
            &self,
            _host: &dyn ToolHost,
            _request: ToolInvocationRequest,
        ) -> std::result::Result<ToolInvocationResponse, CapabilityError> {
            self.1.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(ToolInvocationResponse {
                content: "executed".to_string(),
                is_error: false,
                structured_content: None,
            })
        }
    }

    fn tool_set(config: &Config, base_dir: &Path, tool: ActiveTool) -> ToolSet {
        let active = ActiveToolSet {
            tools: BTreeMap::from([(tool.target_id.name().to_string(), tool)]),
            floor: floor(config, base_dir).unwrap(),
        };
        ToolSet::from_dynamic_tools(active.into_rig_tools())
    }

    #[tokio::test]
    async fn builtin_interaction_routes_through_the_host_context() {
        let config = Config::default();
        let active = ActiveToolSet::builtins(&config, &std::env::temp_dir()).unwrap();
        let tools = ToolSet::from_dynamic_tools(active.into_rig_tools());
        let mut context = ToolContext::new();
        context.insert(QuestionPort::new(AnswerPort));

        let result = tools
            .execute("ask_user", r#"{"question":"Continue?"}"#, &mut context)
            .await;

        assert_eq!(result.output().as_text(), Some("host answer"));
    }

    #[tokio::test]
    async fn approved_external_addition_executes_through_the_host_boundary() {
        let root = std::env::temp_dir();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:external".parse().unwrap(),
            description: "external fixture".to_string(),
            argument_schema: serde_json::json!({"type": "object"}),
            prompt_guidance: String::new(),
            effects: Vec::new(),
        };
        let tools = tool_set(
            &Config::default(),
            &root,
            ActiveTool {
                target_id: "tool:external".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let capability = ApprovalCapability::new(false, Arc::new(ApprovalSink));
        capability.grant_once("external", &serde_json::json!({"value": 1}));
        let mut context = approval_context(capability);
        let result = tools.execute("external", r#"{"value":1}"#, &mut context).await;

        assert_eq!(result.output().as_text(), Some("executed"));
        assert!(executed.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn schema_and_workspace_floor_reject_before_external_execution() {
        let root = std::env::temp_dir().join(format!("dispatch_root_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:external-write".parse().unwrap(),
            description: "write fixture".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
            prompt_guidance: String::new(),
            effects: vec![OperationEffect::WritePath {
                scope: PathScope::Workspace,
            }],
        };
        let tools = tool_set(
            &Config::default(),
            &root,
            ActiveTool {
                target_id: "tool:external-write".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let mut context = ToolContext::new();

        let malformed = tools.execute("external-write", "{}", &mut context).await;
        assert!(malformed.is_error_kind(ToolErrorKind::InvalidArgs));
        let escaping = tools
            .execute("external-write", r#"{"path":"../outside"}"#, &mut context)
            .await;
        assert!(escaping.is_error_kind(ToolErrorKind::PermissionDenied));
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_escape_is_denied_before_external_execution() {
        let root = std::env::temp_dir().join(format!("dispatch_symlink_{}", uuid::Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("dispatch_outside_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:external-write".parse().unwrap(),
            description: "write fixture".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
            prompt_guidance: String::new(),
            effects: vec![OperationEffect::WritePath {
                scope: PathScope::Workspace,
            }],
        };
        let tools = tool_set(
            &Config::default(),
            &root,
            ActiveTool {
                target_id: "tool:external-write".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let result = tools
            .execute(
                "external-write",
                r#"{"path":"escape/out.txt"}"#,
                &mut ToolContext::new(),
            )
            .await;

        assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[tokio::test]
    async fn protected_host_storage_is_denied_before_read_dispatch() {
        let root = std::env::temp_dir().join(format!("dispatch_protected_{}", uuid::Uuid::new_v4()));
        let protected = root.join("rho");
        std::fs::create_dir_all(&protected).unwrap();
        let config = Config {
            config_dir: protected.clone(),
            ..Config::default()
        };
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:external-read".parse().unwrap(),
            description: "read fixture".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
            prompt_guidance: String::new(),
            effects: vec![OperationEffect::ReadPath {
                scope: PathScope::Explicit,
            }],
        };
        let tools = tool_set(
            &config,
            &root,
            ActiveTool {
                target_id: "tool:external-read".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let arguments = serde_json::json!({"path": protected.join("credentials.json")}).to_string();
        let result = tools
            .execute("external-read", &arguments, &mut ToolContext::new())
            .await;

        assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn replacement_cannot_weaken_builtin_safety_floor() {
        let root = std::env::temp_dir().join(format!("dispatch_replace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:replacement-write".parse().unwrap(),
            description: "replacement".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {"path": {"type": "string"}}
            }),
            prompt_guidance: String::new(),
            effects: Vec::new(),
        };
        let tools = tool_set(
            &Config::default(),
            &root,
            ActiveTool {
                target_id: "tool:write".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let result = tools
            .execute("write", r#"{"path":"../outside"}"#, &mut ToolContext::new())
            .await;

        assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn private_network_floor_rejects_before_external_execution() {
        let root = std::env::temp_dir();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descriptor = ToolDescriptor {
            id: "tool:external-fetch".parse().unwrap(),
            description: "fetch fixture".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "required": ["url"],
                "properties": {"url": {"type": "string"}}
            }),
            prompt_guidance: String::new(),
            effects: vec![OperationEffect::Network {
                access: NetworkAccess::PublicInternet,
            }],
        };
        let tools = tool_set(
            &Config::default(),
            &root,
            ActiveTool {
                target_id: "tool:external-fetch".parse().unwrap(),
                descriptor: descriptor.clone(),
                capability: Arc::new(FixtureTool(descriptor, Arc::clone(&executed))),
            },
        );
        let result = tools
            .execute(
                "external-fetch",
                r#"{"url":"http://127.0.0.1/private"}"#,
                &mut ToolContext::new(),
            )
            .await;
        assert!(result.is_error_kind(ToolErrorKind::PermissionDenied));
        assert!(!executed.load(std::sync::atomic::Ordering::Relaxed));
    }
}
