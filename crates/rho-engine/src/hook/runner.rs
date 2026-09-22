use super::types::{HookAction, HookEvent};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(5);

pub fn parse_hook_output(
    status_success: bool,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<HookAction, String> {
    let stdout_str = String::from_utf8_lossy(stdout).trim().to_string();

    if !status_success {
        if let Ok(action) = serde_json::from_str::<HookAction>(&stdout_str) {
            return Ok(action);
        }
        let stderr_str = String::from_utf8_lossy(stderr).trim().to_string();
        return Err(format!(
            "Hook exited with status {}: {stderr_str}",
            exit_code.unwrap_or(-1)
        ));
    }

    if stdout_str.is_empty() {
        return Ok(HookAction::Continue);
    }

    serde_json::from_str::<HookAction>(&stdout_str)
        .map_err(|e| format!("Failed to parse hook response: {e}; raw output: {stdout_str}"))
}

pub async fn run_hook(
    executable: &Path,
    event: &HookEvent,
    working_dir: &Path,
    timeout: Duration,
) -> Result<HookAction, String> {
    let mut cmd = Command::new(executable);
    cmd.current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    crate::process::isolate_group(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn hook {}: {e}", executable.display()))?;

    let pid = child.id();
    let event_json = serde_json::to_string(event).map_err(|e| e.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            let _ = stdin.write_all(event_json.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.flush().await;
        });
    }

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(format!("Hook process error: {e}")),
        Err(_) => {
            if let Some(pid) = pid {
                crate::process::kill_group_by_pid(pid);
            }
            return Err(format!("Hook timed out after {}s", timeout.as_secs()));
        }
    };

    parse_hook_output(
        output.status.success(),
        output.status.code(),
        &output.stdout,
        &output.stderr,
    )
}
