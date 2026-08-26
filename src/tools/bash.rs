use crate::error::AppError;
use crate::tools::approval::enforce_approval;
use crate::tools::bash_ast::{RiskTier, analyze_command_safety};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

pub const DEFAULT_BASH_TIMEOUT_SEC: u64 = 30;
pub const MAX_BASH_BYTES: usize = 50 * 1024; // 50 KB
pub const MAX_BASH_LINES: usize = 2000;

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BashArgs {
    /// Command to execute
    pub command: String,
    /// Timeout in seconds (default: 30)
    pub timeout: Option<u64>,
}

pub struct BashTool {
    pub base_dir: PathBuf,
}

impl BashTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: BashArgs) -> Result<ToolResult, AppError> {
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

        let base = std::env::current_dir().unwrap_or_else(|_| self.base_dir.clone());
        cmd.current_dir(&base);
        cmd.stdin(Stdio::null());
        cmd.kill_on_drop(true);

        let output_future = cmd.output();
        let timeout_future = tokio::time::timeout(Duration::from_secs(timeout_sec), output_future);

        let output = match timeout_future.await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Ok(ToolResult::error(format!(
                    "Failed to spawn process for command '{}': {e}",
                    args.command
                )));
            }
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "Command '{}' timed out after {} seconds",
                    args.command, timeout_sec
                )));
            }
        };

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        let mut combined = String::new();
        if !stdout_str.is_empty() {
            combined.push_str(&stdout_str);
        }
        if !stderr_str.is_empty() {
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&stderr_str);
        }

        let truncated_output = truncate_bash_output(&combined);

        if output.status.success() {
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
}

pub fn is_read_only_command(command: &str) -> bool {
    analyze_command_safety(command).tier == RiskTier::ReadOnly
}

fn truncate_bash_output(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() > MAX_BASH_LINES || output.len() > MAX_BASH_BYTES {
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
            "\n[Output truncated: {} total lines, {} total bytes]",
            lines.len(),
            output.len()
        ));
        truncated
    } else {
        output.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_read_only_command() {
        assert!(is_read_only_command("ls -la"));
        assert!(is_read_only_command("cat Cargo.toml && ls -la"));
        assert!(is_read_only_command("git status"));
        assert!(is_read_only_command("git diff"));
        assert!(is_read_only_command("cargo check"));
        assert!(is_read_only_command("cargo test"));
        assert!(is_read_only_command("rg 'fn main' src/"));

        // Dangerous / mutating commands must return false
        assert!(!is_read_only_command("rm -rf target"));
        assert!(!is_read_only_command("echo 'foo' > file.txt"));
        assert!(!is_read_only_command("git commit -m 'test'"));
        assert!(!is_read_only_command("npm install"));
        assert!(!is_read_only_command("cargo run"));
        assert!(!is_read_only_command("env sh -c 'touch marker'"));
        assert!(!is_read_only_command("git branch new-branch"));
        assert!(!is_read_only_command("git config user.name model"));
        assert!(is_read_only_command("git branch --show-current"));
        assert!(is_read_only_command("git config --get user.name"));
    }

    #[test]
    fn test_bash_output_truncation_preserves_limits() {
        let output = format!("{}\n", "x".repeat(MAX_BASH_BYTES + 100));
        let truncated = truncate_bash_output(&output);
        assert!(truncated.contains("[Output truncated:"));
        assert!(truncated.len() <= MAX_BASH_BYTES + 100);
    }

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new(std::env::current_dir().unwrap());
        let res = tool
            .execute(BashArgs {
                command: "echo 'hello from bash'".to_string(),
                timeout: Some(5),
            })
            .await
            .unwrap();

        assert!(!res.is_error);
        assert!(res.content.contains("hello from bash"));
    }

    #[tokio::test]
    async fn test_bash_nonzero_exit() {
        let tool = BashTool::new(std::env::current_dir().unwrap());
        let res = tool
            .execute(BashArgs {
                command: "exit 42".to_string(),
                timeout: Some(5),
            })
            .await
            .unwrap();

        assert!(res.is_error);
        assert!(res.content.contains("exited with code 42"));
    }

    #[tokio::test]
    async fn test_bash_timeout() {
        let tool = BashTool::new(std::env::current_dir().unwrap());
        let res = tool
            .execute(BashArgs {
                command: "sleep 3".to_string(),
                timeout: Some(1),
            })
            .await
            .unwrap();

        assert!(res.is_error);
        assert!(res.content.contains("timed out after 1 seconds"));
    }
}
