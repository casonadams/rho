use crate::engine::runner::turn::runner::{DispatchDecision, evaluate_tool_dispatch};
use crate::tools::engine_tool::TurnToolContext;
use rho_harness_core::model::{ChatMessage, ToolCall, ToolFunction, ToolResultContent, UserContent};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const FAILURE_TAIL_CHARS: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellOutcome {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ShellOutcome {
    pub(crate) fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    pub(crate) fn failure_summary(&self, command: &str) -> String {
        let code = self.exit_code.map_or_else(
            || "was terminated by a signal".to_string(),
            |c| format!("exited with code {c}"),
        );
        let combined = format!("{}{}", self.stdout, self.stderr);
        format!(
            "`{command}` {code}. Output tail:\n{}",
            tail(&combined, FAILURE_TAIL_CHARS)
        )
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.trim_end().to_string();
    }
    let kept: String = text.chars().skip(total - max_chars).collect();
    format!("[... {} characters omitted]\n{}", total - max_chars, kept.trim_end())
}

fn denial_text(message: &ChatMessage) -> String {
    let ChatMessage::User { content } = message else {
        return "denied".to_string();
    };
    content
        .iter()
        .filter_map(|c| match c {
            UserContent::ToolResult(result) => Some(result),
            _ => None,
        })
        .flat_map(|result| &result.content)
        .find_map(|item| match item {
            ToolResultContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "denied".to_string())
}

/// Runs `command` through the same hook and permission checks as the model's `bash` tool.
async fn approve(ctx: &TurnToolContext, command: &str) -> Result<String, String> {
    let call = ToolCall::new(
        "workflow",
        ToolFunction::new("bash", serde_json::json!({ "command": command })),
    );
    match evaluate_tool_dispatch(&call, ctx.lifecycle.as_ref(), ctx.permission.as_ref()).await {
        DispatchDecision::Proceed(args) => Ok(args
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or(command)
            .to_string()),
        DispatchDecision::Deny(message) | DispatchDecision::Stop(message, _) => Err(denial_text(&message)),
    }
}

async fn wait_output(
    ctx: &TurnToolContext,
    mut command: tokio::process::Command,
    timeout: Duration,
) -> Result<ShellOutcome, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().map_err(|e| format!("failed to start command: {e}"))?;
    let output = child.wait_with_output();
    let timed = tokio::time::timeout(timeout, output);
    let result = match &ctx.cancellation {
        Some(cancel) => tokio::select! {
            res = timed => res,
            () = cancel.wait_cancelled() => return Err("cancelled by operator".to_string()),
        },
        None => timed.await,
    };
    let output = result
        .map_err(|_| format!("timed out after {}s", timeout.as_secs()))?
        .map_err(|e| format!("command failed: {e}"))?;
    Ok(ShellOutcome {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub(crate) async fn run_shell(
    ctx: &TurnToolContext,
    cwd: &Path,
    command: &str,
    timeout: Duration,
) -> Result<ShellOutcome, String> {
    let approved = approve(ctx, command).await?;
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&approved).current_dir(cwd);
    wait_output(ctx, cmd, timeout).await
}

pub(crate) async fn run_git(ctx: &TurnToolContext, cwd: &Path, args: &[&str]) -> Result<ShellOutcome, String> {
    let display = std::iter::once("git")
        .chain(args.iter().copied())
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ");
    approve(ctx, &display).await?;
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(cwd);
    wait_output(ctx, cmd, Duration::from_secs(120)).await
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@".contains(c)) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_commands_and_reports_failures_with_output_tail() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = TurnToolContext::default();
        let ok = run_shell(&ctx, dir.path(), "echo hi", Duration::from_secs(5))
            .await
            .unwrap();
        assert!(ok.success());
        assert_eq!(ok.stdout, "hi\n");

        let failed = run_shell(&ctx, dir.path(), "echo boom >&2; exit 3", Duration::from_secs(5))
            .await
            .unwrap();
        assert!(!failed.success());
        let summary = failed.failure_summary("make all");
        assert!(summary.starts_with("`make all` exited with code 3."), "{summary}");
        assert!(summary.ends_with("boom"), "{summary}");
    }

    #[test]
    fn quotes_only_arguments_that_need_it() {
        assert_eq!(shell_quote("commit"), "commit");
        assert_eq!(shell_quote("fix(x): it's done"), "'fix(x): it'\\''s done'");
    }

    #[test]
    fn tail_keeps_the_end_of_long_output() {
        let text = format!("{}END", "a".repeat(FAILURE_TAIL_CHARS));
        let cut = tail(&text, FAILURE_TAIL_CHARS);
        assert!(cut.starts_with("[... 3 characters omitted]"));
        assert!(cut.ends_with("END"));
    }
}
