use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};

pub const MAX_STDERR_BYTES: usize = 64 * 1024;

pub struct McpChildHandle {
    pub stderr_buffer: Arc<Mutex<String>>,
    _guard: crate::process::ProcessTreeGuard,
}

impl McpChildHandle {
    pub fn id(&self) -> Option<u32> {
        self._guard.id()
    }

    pub fn last_stderr(&self) -> String {
        self.stderr_buffer.lock().unwrap().clone()
    }
}

pub struct McpProcess;

fn build_mcp_command(config: &McpServerConfig, working_dir: &Path) -> Result<Command> {
    let command_str = config
        .command
        .as_deref()
        .ok_or_else(|| AppError::Mcp("No command specified for stdio MCP server".to_string()))?;
    let mut cmd = Command::new(command_str);
    cmd.args(&config.args);
    cmd.current_dir(working_dir);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    crate::process::isolate_group(&mut cmd);
    for (key, val) in resolve_env(&config.env) {
        cmd.env(key, val);
    }
    Ok(cmd)
}

fn spawn_stderr_reader(stderr: ChildStderr) -> Arc<Mutex<String>> {
    let stderr_buffer = Arc::new(Mutex::new(String::new()));
    let buffer_clone = Arc::clone(&stderr_buffer);
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let mut buf = buffer_clone.lock().unwrap();
            if buf.len() < MAX_STDERR_BYTES {
                buf.push_str(&line);
                buf.push('\n');
            }
        }
    });
    stderr_buffer
}

fn take_process_stdio(child: &mut tokio::process::Child) -> Result<(ChildStdin, ChildStdout, ChildStderr)> {
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Mcp("Failed to open child process stdin".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Mcp("Failed to open child process stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Mcp("Failed to open child process stderr".to_string()))?;
    Ok((stdin, stdout, stderr))
}

impl McpProcess {
    pub fn spawn(config: &McpServerConfig, working_dir: &Path) -> Result<(ChildStdin, ChildStdout, McpChildHandle)> {
        let command_name = config.command.as_deref().unwrap_or("<unspecified>");
        let mut cmd = build_mcp_command(config, working_dir)?;
        let mut child = cmd
            .spawn()
            .map_err(|error| AppError::Mcp(format!("Failed to spawn MCP server '{command_name}': {error}")))?;

        let (stdin, stdout, stderr) = take_process_stdio(&mut child)?;
        let stderr_buffer = spawn_stderr_reader(stderr);

        Ok((
            stdin,
            stdout,
            McpChildHandle {
                stderr_buffer,
                _guard: crate::process::ProcessTreeGuard::new(child),
            },
        ))
    }
}

pub fn resolve_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut resolved = BTreeMap::new();
    for (key, val) in env {
        if let Some(var_name) = val.strip_prefix("env:") {
            if let Ok(env_val) = std::env::var(var_name) {
                resolved.insert(key.clone(), env_val);
            }
        } else {
            resolved.insert(key.clone(), val.clone());
        }
    }
    resolved
}

#[cfg(test)]
mod tests;
