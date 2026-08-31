//! Host platform assembly: the application loads the active tool platform
//! (built-in tools + configured plugins + MCP) and hands the prepared forms
//! to the engine.

use async_trait::async_trait;
use rho_core::config::Config;
use rho_core::error::Result;
use rho_core::presentation::{
    ActivityToken, ApprovalResult, BashApproval, Presenter, SessionStatus, StructuredPresenter, ToolLine,
    WelcomeDisplay, activity_token,
};
use rho_engine::auth::AuthStore;
use rho_engine::engine::{AgentEngine, builder::AgentEngineBuilder};
use rho_host::tool_dispatch::ActiveToolSet;
use rho_plugin_builtin::subagents::{AgentExecutionResult, SubagentExecuteRequest, SubagentExecutor};
use rho_sdk::contract::{CommandCapability, ContextCapability, LifecycleCapability, ToolHost};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct ToolAssembly {
    pub rig_tools: Vec<rig::tool::DynamicTool>,
    pub neutral_executor: Arc<dyn rho_core::dispatch::NeutralToolExecutor>,
    pub contexts: Vec<Arc<dyn ContextCapability>>,
    pub commands: BTreeMap<String, Arc<dyn CommandCapability>>,
    pub lifecycles: Vec<Arc<dyn LifecycleCapability>>,
}

pub struct AppSubagentExecutor {
    config: Config,
    auth_store: AuthStore,
    base_dir: PathBuf,
    tool_platform: tokio::sync::OnceCell<Arc<ActiveToolSet>>,
}

impl AppSubagentExecutor {
    pub fn new(config: Config, auth_store: AuthStore, base_dir: PathBuf) -> Self {
        Self {
            config,
            auth_store,
            base_dir,
            tool_platform: tokio::sync::OnceCell::new(),
        }
    }

    async fn shared_tool_platform(&self) -> Result<Arc<ActiveToolSet>> {
        self.tool_platform
            .get_or_try_init(|| async {
                // The platform depends on directories, network policy, and
                // plugin/MCP config only; model overrides are engine-level.
                // A failed load leaves the cell uninitialized to retry later.
                ActiveToolSet::load(&self.config, &self.base_dir).await.map(Arc::new)
            })
            .await
            .cloned()
    }
}

struct SubagentHostPresenter {
    inner: StructuredPresenter,
    chunk_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

#[async_trait]
impl Presenter for SubagentHostPresenter {
    fn write_output(&self, text: &str) {
        let _ = self.chunk_tx.send(text.to_string());
    }

    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_user_block(&self, _input: &str) {}

    fn print_token(&self, token: &str) {
        let _ = self.chunk_tx.send(token.to_string());
    }

    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        false
    }
    fn start_spinner(&self, _message: &str) -> ActivityToken {
        activity_token(|| {})
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        activity_token(|| {})
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> rho_core::presentation::stream::ToolStreamPort {
        self.inner.stream_port()
    }
    fn question_port(&self) -> rho_core::presentation::questions::QuestionPort {
        self.inner.question_port()
    }
    async fn prompt_tool_approval(&self, _name: &str, _arguments: &serde_json::Value) -> ApprovalResult {
        ApprovalResult::Approved
    }
    async fn prompt_bash_approval(&self, _request: BashApproval) -> ApprovalResult {
        ApprovalResult::Approved
    }
    async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
        false
    }
}

struct SubagentSteeringQueue {
    receiver: Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>>>,
}

#[async_trait]
impl rho_engine::engine::provider::host_loop::SteeringQueueProvider for SubagentSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        let Some(rx) = &self.receiver else {
            return Vec::new();
        };
        let mut guard = rx.lock().await;
        let mut messages = Vec::new();
        while let Ok(msg) = guard.try_recv() {
            messages.push(msg);
        }
        messages
    }
}

#[async_trait]
impl SubagentExecutor for AppSubagentExecutor {
    async fn execute(&self, request: SubagentExecuteRequest<'_>, host: &dyn ToolHost) -> Result<AgentExecutionResult> {
        let mut subagent_config = self.config.clone();
        if let Some(model) = request.model_override.or(request.template.model.as_deref()) {
            subagent_config.model = model.to_string();
        }
        subagent_config.auto_approve = true;

        let active_tools = self.shared_tool_platform().await?;
        let rig_tools = ActiveToolSet::clone(&active_tools).into_rig_tools();
        let neutral_executor = Arc::new(active_tools.neutral_executor(rig::tool::ToolContext::default()));

        let engine = AgentEngineBuilder::new(subagent_config, self.auth_store.clone())
            .base_dir(self.base_dir.clone())
            .tool_assembly(rig_tools, neutral_executor)
            .build()
            .await?;

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let presenter: Arc<dyn Presenter> = Arc::new(SubagentHostPresenter {
            inner: StructuredPresenter::stdout(),
            chunk_tx,
        });
        let prompt_with_instructions = format!("{}\n\nTask:\n{}", request.template.system_prompt, request.prompt);

        let steering_queue = SubagentSteeringQueue {
            receiver: request.steering_rx.clone(),
        };

        let turn_future = engine.run_turn(
            rho_engine::engine::runner::TurnRequest {
                prompt: &prompt_with_instructions,
                cancellation: None,
                steering: Some(&steering_queue),
            },
            presenter,
        );

        tokio::pin!(turn_future);

        let turn_res = loop {
            tokio::select! {
                res = &mut turn_future => break res,
                Some(chunk) = chunk_rx.recv() => {
                    host.stream_chunk(&chunk);
                }
            }
        };

        // Drain any remaining chunks
        while let Ok(chunk) = chunk_rx.try_recv() {
            host.stream_chunk(&chunk);
        }

        match turn_res {
            Ok(output) => Ok(AgentExecutionResult {
                job_id: request.job_id.unwrap_or_default().to_string(),
                status: "completed".to_string(),
                text: output.final_text,
                tool_calls_count: output.tool_calls_count,
                is_error: false,
            }),
            Err(err) => Ok(AgentExecutionResult {
                job_id: request.job_id.unwrap_or_default().to_string(),
                status: "failed".to_string(),
                text: format!("Subagent turn failed: {err}"),
                tool_calls_count: 0,
                is_error: true,
            }),
        }
    }
}

/// Assemble the active tool platform for a config, including configured
/// external plugins and MCP servers.
pub async fn active_tools(config: &Config, base_dir: &Path) -> Result<ToolAssembly> {
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    active_tools_with_auth(config, base_dir, &auth_store).await
}

pub async fn active_tools_with_auth(config: &Config, base_dir: &Path, auth_store: &AuthStore) -> Result<ToolAssembly> {
    let executor: Option<Arc<dyn SubagentExecutor>> = Some(Arc::new(AppSubagentExecutor::new(
        config.clone(),
        auth_store.clone(),
        base_dir.to_path_buf(),
    )));

    let tool_set = std::sync::Arc::new(ActiveToolSet::load_with_executor(config, base_dir, executor).await?);
    let neutral_executor = tool_set.neutral_executor(rig::tool::ToolContext::default());
    let rig_tools = ActiveToolSet::clone(&tool_set).into_rig_tools();
    let contexts = tool_set.active_contexts();
    let commands = tool_set.active_commands();
    let lifecycles = tool_set.active_lifecycles();
    Ok(ToolAssembly {
        rig_tools,
        neutral_executor: std::sync::Arc::new(neutral_executor),
        contexts,
        commands,
        lifecycles,
    })
}

impl ToolAssembly {
    pub fn into_parts(
        self,
    ) -> (
        Vec<rig::tool::DynamicTool>,
        Arc<dyn rho_core::dispatch::NeutralToolExecutor>,
    ) {
        (self.rig_tools, self.neutral_executor)
    }
}

/// Build the interactive application engine with the platform injected.
pub async fn agent_engine(config: Config, auth_store: AuthStore, resume: Option<&str>) -> Result<AgentEngine> {
    let base_dir = std::env::current_dir()?;
    let assembly = active_tools_with_auth(&config, &base_dir, &auth_store).await?;
    AgentEngineBuilder::new(config, auth_store)
        .resume(resume)
        .base_dir(base_dir)
        .contexts(assembly.contexts)
        .lifecycles(assembly.lifecycles)
        .tool_assembly(assembly.rig_tools, assembly.neutral_executor)
        .build()
        .await
}
