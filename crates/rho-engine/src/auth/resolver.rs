//! Dynamic key expansion: environment variables, shell commands (!command), and escapes.

use rho_harness_core::error::{AppError, Result};
use std::io::Read;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const SECRET_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub fn resolve_secret_value(raw: &str) -> Result<String> {
    classify_secret(raw)?
        .literal()
        .or_else(|cmd| execute_command_with_timeout(cmd, SECRET_COMMAND_TIMEOUT))
}

pub async fn resolve_secret_value_async(raw: &str) -> Result<String> {
    match classify_secret(raw)?.literal() {
        Ok(value) => Ok(value),
        Err(cmd) => execute_command_async_with_timeout(cmd, SECRET_COMMAND_TIMEOUT).await,
    }
}

enum SecretSource<'a> {
    Value(String),
    Command(&'a str),
}

impl<'a> SecretSource<'a> {
    /// The resolved value, or the shell command that still has to run to produce it.
    fn literal(self) -> std::result::Result<String, &'a str> {
        match self {
            SecretSource::Value(value) => Ok(value),
            SecretSource::Command(cmd) => Err(cmd),
        }
    }
}

fn classify_secret(raw: &str) -> Result<SecretSource<'_>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(SecretSource::Value(String::new()));
    }

    if let Some(rest) = trimmed.strip_prefix("$$") {
        return Ok(SecretSource::Value(format!("${rest}")));
    }
    if let Some(rest) = trimmed.strip_prefix("$!") {
        return Ok(SecretSource::Value(format!("!{rest}")));
    }

    if let Some(cmd) = trimmed.strip_prefix('!') {
        return Ok(SecretSource::Command(cmd));
    }

    if trimmed.starts_with('$') {
        return expand_env(trimmed).map(SecretSource::Value);
    }

    Ok(SecretSource::Value(trimmed.to_string()))
}

fn shell_command(cmd: &str) -> Command {
    #[cfg(unix)]
    let mut command = Command::new("sh");
    #[cfg(unix)]
    command.arg("-c").arg(cmd);
    #[cfg(not(unix))]
    let mut command = Command::new("cmd");
    #[cfg(not(unix))]
    command.arg("/C").arg(cmd);
    command
}

fn timed_out(cmd: &str, timeout: Duration) -> AppError {
    AppError::Auth(format!(
        "Auth command '{cmd}' timed out after {}ms",
        timeout.as_millis()
    ))
}

fn execute_command_with_timeout(cmd: &str, timeout: Duration) -> Result<String> {
    let io_error = |e: std::io::Error| AppError::Auth(format!("Failed to execute auth command '{cmd}': {e}"));
    let mut child = shell_command(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_error)?;
    // Drain both pipes concurrently so a child writing more than the pipe buffer cannot block on write.
    let stdout_reader = child.stdout.take().map(drain_pipe);
    let stderr_reader = child.stderr.take().map(drain_pipe);
    let deadline = Instant::now() + timeout;
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait().map_err(io_error)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(timed_out(cmd, timeout));
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    // A backgrounded grandchild can keep the pipes open after the child exits, so the reads share the deadline.
    let stdout = collect_reader(stdout_reader, deadline).ok_or_else(|| timed_out(cmd, timeout))?;
    let stderr = collect_reader(stderr_reader, deadline).ok_or_else(|| timed_out(cmd, timeout))?;
    let output = Output { status, stdout, stderr };
    validate_command_output(output, cmd)
}

fn drain_pipe<R: Read + Send + 'static>(mut pipe: R) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

fn collect_reader(reader: Option<Receiver<Vec<u8>>>, deadline: Instant) -> Option<Vec<u8>> {
    let Some(rx) = reader else {
        return Some(Vec::new());
    };
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(buf) => Some(buf),
        Err(RecvTimeoutError::Disconnected) => Some(Vec::new()),
        Err(RecvTimeoutError::Timeout) => None,
    }
}

fn validate_command_output(output: std::process::Output, cmd: &str) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Auth(format!(
            "Auth command '{cmd}' failed (exit code {:?}): {}",
            output.status.code(),
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err(AppError::Auth(format!("Auth command '{cmd}' returned empty output")));
    }
    Ok(stdout)
}

async fn execute_command_async_with_timeout(cmd: &str, timeout: Duration) -> Result<String> {
    let mut command = tokio::process::Command::from(shell_command(cmd));
    command.kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| timed_out(cmd, timeout))?
        .map_err(|e| AppError::Auth(format!("Failed to execute auth command '{cmd}': {e}")))?;
    validate_command_output(output, cmd)
}

fn extract_braced_var<I: Iterator<Item = (usize, char)>>(chars: &mut I) -> String {
    let mut var_name = String::new();
    for (_, vc) in chars {
        if vc == '}' {
            break;
        }
        var_name.push(vc);
    }
    var_name
}

fn extract_unbraced_var<I: std::iter::Iterator<Item = (usize, char)>>(chars: &mut std::iter::Peekable<I>) -> String {
    let mut var_name = String::new();
    while let Some(&(_, vc)) = chars.peek() {
        if vc.is_alphanumeric() || vc == '_' {
            var_name.push(vc);
            chars.next();
        } else {
            break;
        }
    }
    var_name
}

fn resolve_env_var_name<I: std::iter::Iterator<Item = (usize, char)>>(
    chars: &mut std::iter::Peekable<I>,
) -> Result<String> {
    let var_name = if let Some(&(_, '{')) = chars.peek() {
        chars.next();
        extract_braced_var(chars)
    } else {
        extract_unbraced_var(chars)
    };
    if var_name.is_empty() {
        return Ok("$".to_string());
    }
    std::env::var(&var_name).map_err(|_| AppError::Auth(format!("Environment variable '{var_name}' is not set")))
}

fn expand_env(s: &str) -> Result<String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '$' {
            result.push_str(&resolve_env_var_name(&mut chars)?);
        } else {
            result.push(c);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_escapes_properly() {
        assert_eq!(resolve_secret_value("$$FOO").unwrap(), "$FOO");
        assert_eq!(resolve_secret_value("$!BAR").unwrap(), "!BAR");
    }

    #[test]
    fn resolves_literals_as_is() {
        assert_eq!(resolve_secret_value("sk-ant-12345").unwrap(), "sk-ant-12345");
    }

    #[test]
    fn expands_environment_variables() {
        unsafe { std::env::set_var("RHO_TEST_SECRET_KEY", "secret_val_42") };
        assert_eq!(resolve_secret_value("$RHO_TEST_SECRET_KEY").unwrap(), "secret_val_42");
        assert_eq!(resolve_secret_value("${RHO_TEST_SECRET_KEY}").unwrap(), "secret_val_42");
    }

    #[test]
    fn executes_shell_commands() {
        assert_eq!(
            resolve_secret_value("!echo hello_from_shell").unwrap(),
            "hello_from_shell"
        );
    }

    #[tokio::test]
    async fn async_resolution_matches_sync_for_literals_and_commands() {
        assert_eq!(resolve_secret_value_async("sk-literal").await.unwrap(), "sk-literal");
        assert_eq!(
            resolve_secret_value_async("!echo async_secret").await.unwrap(),
            "async_secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sync_command_with_large_output_does_not_stall_until_timeout() {
        let value = execute_command_with_timeout("yes ok | head -c 131072", Duration::from_secs(5)).unwrap();
        assert!(value.starts_with("ok\nok"));
    }

    #[cfg(unix)]
    #[test]
    fn sync_command_with_grandchild_holding_stdout_respects_timeout() {
        let started = Instant::now();
        let err = execute_command_with_timeout("sleep 2 & echo done", Duration::from_millis(200)).unwrap_err();
        assert!(err.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failing_or_hung_secret_commands_are_errors_in_both_modes() {
        let failed = execute_command_async_with_timeout("echo nope >&2; exit 3", Duration::from_secs(5)).await;
        assert!(failed.unwrap_err().to_string().contains("nope"));

        let hung = execute_command_with_timeout("sleep 5", Duration::from_millis(50)).unwrap_err();
        assert!(hung.to_string().contains("timed out"));
        let hung = execute_command_async_with_timeout("sleep 5", Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(hung.to_string().contains("timed out"));
    }
}
