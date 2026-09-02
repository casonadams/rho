use rho_harness_core::config::PluginConfig;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::{ApprovalResult, InteractionPrompt};
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::task::JoinHandle;

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
struct HookCapabilities {
    ui_prompt: bool,
}

#[derive(Debug, Serialize)]
struct PreToolCallPayload<'a> {
    event: &'static str,
    tool: &'a str,
    arguments: &'a Value,
    capabilities: HookCapabilities,
}

#[derive(Debug, Serialize)]
struct PostToolResultPayload<'a> {
    event: &'static str,
    tool: &'a str,
    arguments: &'a Value,
    output: &'a str,
    is_error: bool,
}

#[derive(Debug, Deserialize)]
struct HookResponse {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Denials surface as `Err(reason)`; `Ok` decisions continue to the UI layer.
#[derive(Debug, PartialEq, Eq)]
enum HookDecision {
    Allow,
    Ask,
}

#[derive(Clone)]
pub struct PluginHook {
    plugins: Vec<(String, PluginConfig)>,
    working_dir: PathBuf,
    presenter: Arc<dyn Presenter>,
}

impl PluginHook {
    pub fn new(
        plugins: &BTreeMap<String, PluginConfig>,
        working_dir: impl AsRef<Path>,
        presenter: Arc<dyn Presenter>,
    ) -> Self {
        let active = plugins
            .iter()
            .filter(|(_, p)| p.enabled && (!p.path.as_os_str().is_empty() || p.command.is_some()))
            .map(|(name, p)| (name.clone(), p.clone()))
            .collect();
        Self {
            plugins: active,
            working_dir: working_dir.as_ref().to_path_buf(),
            presenter,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

impl AgentHook for PluginHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let args = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        let payload = PreToolCallPayload {
            event: "pre_tool_call",
            tool: event.tool_name,
            arguments: &args,
            capabilities: HookCapabilities {
                ui_prompt: self.presenter.has_interactive_ui(),
            },
        };
        let Ok(payload_json) = serde_json::to_vec(&payload) else {
            return ToolCallAction::run();
        };

        for (name, plugin) in &self.plugins {
            match self.execute(plugin, &payload_json).await {
                Err(reason) => {
                    return ToolCallAction::skip(format!("Plugin '{name}' denied tool execution: {reason}"));
                }
                Ok(HookDecision::Ask) => match self.presenter.prompt_tool_approval(event.tool_name, &args).await {
                    ApprovalResult::Approved | ApprovalResult::ApprovedForSession => continue,
                    ApprovalResult::Denied { reason } => {
                        return ToolCallAction::skip(format!("Permission denied: {reason}"));
                    }
                },
                Ok(HookDecision::Allow) => continue,
            }
        }

        ToolCallAction::run()
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        let args = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        let output = event.presentation.render();
        let payload = PostToolResultPayload {
            event: "post_tool_result",
            tool: event.tool_name,
            arguments: &args,
            output: &output,
            is_error: !event.raw_result.is_success(),
        };

        if let Ok(payload_json) = serde_json::to_vec(&payload) {
            for (_, plugin) in &self.plugins {
                let _ = self.execute(plugin, &payload_json).await;
            }
        }

        ToolResultAction::keep()
    }
}

impl PluginHook {
    /// Runs one plugin for one hook event. While the plugin is deciding, its
    /// `ui/prompt` requests are served through the presenter.
    async fn execute(&self, plugin: &PluginConfig, payload: &[u8]) -> Result<HookDecision, String> {
        let Ok((program, args)) = resolve_executable(plugin, &self.working_dir) else {
            return Ok(HookDecision::Allow);
        };
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&self.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let Ok(mut child) = cmd.spawn() else {
            return Ok(HookDecision::Allow);
        };
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "plugin stdin unavailable".to_string())?;
        write_line(&mut stdin, payload).await?;
        let stderr = stderr_collector(&mut child);

        let decision = self.pump(&mut child, &mut stdin).await;
        drop(stdin);
        match decision {
            Some(answer) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                answer
            }
            None => finish_after_eof(&mut child, stderr).await,
        }
    }

    /// Reads plugin stdout until a hook response arrives, serving `ui/prompt`
    /// requests along the way. `None` when the stream ends without one.
    async fn pump(&self, child: &mut Child, stdin: &mut ChildStdin) -> Option<Result<HookDecision, String>> {
        let mut stdout = BufReader::new(child.stdout.take()?);
        let mut line = String::new();
        loop {
            match stdout.read_line(&mut line).await {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            if let Some(answer) = self.handle_line(&line, stdin).await {
                return Some(answer);
            }
            line.clear();
        }
    }

    async fn handle_line(&self, line: &str, stdin: &mut ChildStdin) -> Option<Result<HookDecision, String>> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return None;
        };
        if value.get("method").and_then(Value::as_str) == Some("ui/prompt") {
            self.serve_ui_prompt(value, stdin).await;
            return None;
        }
        let Ok(response) = serde_json::from_value::<HookResponse>(value) else {
            return None;
        };
        match response.action.as_deref() {
            Some("deny") | Some("block") => Some(Err(deny_reason(response))),
            Some("ask") | Some("prompt") => Some(Ok(HookDecision::Ask)),
            Some(_) => Some(Ok(HookDecision::Allow)),
            None => None,
        }
    }

    async fn serve_ui_prompt(&self, request: Value, stdin: &mut ChildStdin) {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let reply =
            match serde_json::from_value::<InteractionPrompt>(request.get("params").cloned().unwrap_or_default()) {
                Ok(prompt) => self.presenter.request_interaction(prompt).await,
                Err(_) => None,
            };
        let result = reply.map_or(Value::Null, |response| {
            serde_json::to_value(response).unwrap_or(Value::Null)
        });
        let _ = write_line(
            stdin,
            json!({"jsonrpc": "2.0", "id": id, "result": result})
                .to_string()
                .as_bytes(),
        )
        .await;
    }
}

fn deny_reason(response: HookResponse) -> String {
    response
        .reason
        .or(response.error)
        .unwrap_or_else(|| "action denied".to_string())
}

async fn write_line(stdin: &mut ChildStdin, payload: &[u8]) -> Result<(), String> {
    stdin.write_all(payload).await.map_err(|error| error.to_string())?;
    stdin.write_all(b"\n").await.map_err(|error| error.to_string())?;
    stdin.flush().await.map_err(|error| error.to_string())
}

fn stderr_collector(child: &mut Child) -> Option<JoinHandle<String>> {
    child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut buffer = String::new();
            let _ = stderr.read_to_string(&mut buffer).await;
            buffer
        })
    })
}

async fn finish_after_eof(child: &mut Child, stderr: Option<JoinHandle<String>>) -> Result<HookDecision, String> {
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if status.success() {
        return Ok(HookDecision::Allow);
    }
    let details = match stderr {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    let details = details.trim();
    if details.is_empty() {
        return Err(format!("process exited with code {}", status.code().unwrap_or(1)));
    }
    Err(details.to_string())
}

fn resolve_executable(plugin: &PluginConfig, working_dir: &Path) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(cmd) = &plugin.command {
        return Ok((PathBuf::from(cmd), plugin.args.clone()));
    }

    let path = if plugin.path.is_absolute() {
        plugin.path.clone()
    } else {
        working_dir.join(&plugin.path)
    };

    if path.is_file() {
        return Ok((path, plugin.args.clone()));
    }

    let release_bin = path
        .join("target/release")
        .join(plugin.path.file_name().unwrap_or_default());
    if release_bin.is_file() {
        return Ok((release_bin, plugin.args.clone()));
    }

    let debug_bin = path
        .join("target/debug")
        .join(plugin.path.file_name().unwrap_or_default());
    if debug_bin.is_file() {
        return Ok((debug_bin, plugin.args.clone()));
    }

    Ok((path, plugin.args.clone()))
}
