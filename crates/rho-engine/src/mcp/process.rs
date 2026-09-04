use rho_harness_core::config::McpServerConfig;
use rho_harness_core::error::{AppError, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

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

impl McpProcess {
    pub fn spawn(config: &McpServerConfig, working_dir: &Path) -> Result<(ChildStdin, ChildStdout, McpChildHandle)> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        cmd.current_dir(working_dir);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        crate::process::isolate_group(&mut cmd);

        let resolved_env = resolve_env(&config.env);
        for (key, val) in resolved_env {
            cmd.env(key, val);
        }

        let mut child = cmd.spawn().map_err(|error| {
            AppError::Plugin(format!(
                "Failed to spawn MCP server '{}' (command: '{}'): {error}",
                config.command, config.command
            ))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Plugin("Failed to open child process stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Plugin("Failed to open child process stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Plugin("Failed to open child process stderr".to_string()))?;

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
