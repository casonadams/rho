pub mod accumulator;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
pub use accumulator::{OutputAccumulator, OutputSnapshot};
pub use rho_harness_core::args::BashArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[cfg(test)]
mod tests;

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;

pub struct BashTool {
    pub base_dir: PathBuf,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute_streaming<F>(&self, args: BashArgs, mut on_chunk: F) -> Result<ToolResult, AppError>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let timeout_sec = args.timeout.unwrap_or(DEFAULT_BASH_TIMEOUT_SEC);

        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(&args.command);
            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd.exe");
            c.arg("/C").arg(&args.command);
            c
        };

        let base = Workspace::new(&self.base_dir);
        cmd.current_dir(base.root());
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        crate::process::isolate_group(&mut cmd);

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to spawn process for command '{}': {e}",
                    args.command
                )));
            }
        };

        let stdout = child.stdout.take().expect("child stdout was piped");
        let stderr = child.stderr.take().expect("child stderr was piped");
        let mut guard = crate::process::ProcessTreeGuard::new(child);

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let stdout_tx = chunk_tx.clone();
        let stdout_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut reader = stdout;
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let s = String::from_utf8_lossy(&buf[..n]).to_string();
                if stdout_tx.send(s).is_err() {
                    break;
                }
            }
        });

        let stderr_tx = chunk_tx;
        let stderr_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut reader = stderr;
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let s = String::from_utf8_lossy(&buf[..n]).to_string();
                if stderr_tx.send(s).is_err() {
                    break;
                }
            }
        });

        let mut accumulator = OutputAccumulator::new();
        let execution_future = async {
            while let Some(chunk) = chunk_rx.recv().await {
                on_chunk(&chunk);
                accumulator.append(chunk.as_bytes());
            }
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            accumulator.finish();
            guard.wait().await
        };

        let status = match tokio::time::timeout(Duration::from_secs(timeout_sec), execution_future).await {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                return Ok(ToolResult::error(format!(
                    "Failed waiting for command '{}': {e}",
                    args.command
                )));
            }
            Err(_) => {
                guard.kill().await;
                return Ok(ToolResult::error(format!(
                    "Command '{}' timed out after {} seconds",
                    args.command, timeout_sec
                )));
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        let snapshot = accumulator.snapshot();
        let truncated_output = snapshot.formatted_text;

        if status.success() {
            let res = if truncated_output.trim().is_empty() {
                "[Command completed with exit code 0 (no output)]".to_string()
            } else {
                truncated_output
            };
            Ok(ToolResult::success(res))
        } else {
            let res = format!("Command exited with code {exit_code}:\n{truncated_output}");
            Ok(ToolResult::error(res))
        }
    }

    pub async fn execute(&self, args: BashArgs) -> Result<ToolResult, AppError> {
        self.execute_streaming(args, |_| {}).await
    }
}

pub fn is_read_only_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.contains('>') || cmd.contains("$(") || cmd.contains('`') {
        return false;
    }
    let subcommands: Vec<&str> = cmd
        .split([';', '&', '|'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    subcommands.iter().all(|sub| is_single_read_only_command(sub))
}

fn is_single_read_only_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains("-delete") || lower.contains("-exec") {
        return false;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(first) = tokens.first() else {
        return true;
    };
    let exe = first.split('/').next_back().unwrap_or(first).to_ascii_lowercase();

    match exe.as_str() {
        "ls" | "pwd" | "whoami" | "which" | "whereis" | "echo" | "printf" | "cat" | "head" | "tail" | "grep" | "rg"
        | "find" | "wc" | "diff" | "file" | "stat" | "uname" | "printenv" | "true" | "false" => true,
        "git" => {
            if let Some(sub) = tokens.get(1) {
                match *sub {
                    "status" | "diff" | "log" | "show" | "describe" => true,
                    "branch" => tokens
                        .iter()
                        .any(|&t| t == "--show-current" || t == "-a" || t == "-r" || t == "--list" || t == "-l"),
                    "config" => tokens.iter().any(|&t| t == "--get" || t == "--list" || t == "-l"),
                    _ => false,
                }
            } else {
                true
            }
        }
        "cargo" => {
            if let Some(sub) = tokens.get(1) {
                matches!(
                    *sub,
                    "check" | "clippy" | "test" | "fmt" | "tree" | "metadata" | "verify-project" | "read-manifest"
                )
            } else {
                false
            }
        }
        _ => false,
    }
}

impl Tool for BashTool {
    const NAME: &'static str = "bash";
    type Args = BashArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<BashArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
