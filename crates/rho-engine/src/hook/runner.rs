use super::types::{HookAction, HookEvent};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(5);

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
        Err(_) => return Err(format!("Hook timed out after {}s", timeout.as_secs())),
    };

    let stdout_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if !output.status.success() {
        if let Ok(action) = serde_json::from_str::<HookAction>(&stdout_str) {
            return Ok(action);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "Hook exited with status {}: {stderr}",
            output.status.code().unwrap_or(-1)
        ));
    }

    if stdout_str.is_empty() {
        return Ok(HookAction::Continue);
    }

    serde_json::from_str::<HookAction>(&stdout_str)
        .map_err(|e| format!("Failed to parse hook response: {e}; raw output: {stdout_str}"))
}
