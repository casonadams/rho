use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rho_core::approval::enforce_approval;
pub use rho_core::args::BashArgs;
use rho_core::bash_ast::{RiskTier, analyze_command_safety};
use rho_core::error::AppError;
use rho_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[cfg(test)]
mod tests;

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;
pub const MAX_BASH_BYTES: usize = 50 * 1024; // 50 KB
pub const MAX_BASH_LINES: usize = 2000;
/// Live-output retention cap while the child is still running; everything past
/// this point streams to the display but is dropped from the captured result.
pub const MAX_RETAINED_BASH_BYTES: usize = 64 * 1024;

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

        let mut combined = String::new();
        let mut total_bytes = 0_usize;
        let mut total_newlines = 0_usize;
        let mut stream_ends_newline = true;
        let execution_future = async {
            while let Some(chunk) = chunk_rx.recv().await {
                on_chunk(&chunk);
                total_bytes = total_bytes.saturating_add(chunk.len());
                total_newlines += chunk.bytes().filter(|&byte| byte == b'\n').count();
                if !chunk.is_empty() {
                    stream_ends_newline = chunk.ends_with('\n');
                }
                let room = MAX_RETAINED_BASH_BYTES.saturating_sub(combined.len());
                let take = room.min(chunk.len());
                let take = if take == chunk.len() || chunk.is_char_boundary(take) {
                    take
                } else {
                    (0..take).rev().find(|&i| chunk.is_char_boundary(i)).unwrap_or(0)
                };
                if let Some(prefix) = chunk.get(..take) {
                    combined.push_str(prefix);
                }
            }
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            child.wait().await
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
                let _ = child.kill().await;
                return Ok(ToolResult::error(format!(
                    "Command '{}' timed out after {} seconds",
                    args.command, timeout_sec
                )));
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        let total_lines = total_newlines + usize::from(!stream_ends_newline && total_bytes > 0);
        let truncated_output = truncate_bash_output(&combined, total_bytes, total_lines);

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
    analyze_command_safety(command).tier == RiskTier::ReadOnly
}

fn truncate_bash_output(retained: &str, total_bytes: usize, total_lines: usize) -> String {
    let lines: Vec<&str> = retained.lines().collect();
    if total_lines > MAX_BASH_LINES || retained.len() > MAX_BASH_BYTES {
        let keep_lines = lines.len().min(MAX_BASH_LINES);
        let mut truncated = String::new();
        let mut bytes = 0;

        for line in lines[..keep_lines].iter() {
            if bytes + line.len() > MAX_BASH_BYTES {
                break;
            }
            truncated.push_str(line);
            truncated.push('\n');
            bytes += line.len() + 1;
        }

        truncated.push_str(&format!(
            "\n[Output truncated: {total_lines} total lines, {total_bytes} total bytes]",
        ));
        truncated
    } else {
        retained.to_string()
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

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        enforce_approval(context, Self::NAME, &args)?;
        into_rig_result(self.execute(args).await)
    }
}
