//! Dynamic key expansion: environment variables, shell commands (!command), and escapes.

use rho_harness_core::error::{AppError, Result};
use std::process::Command;

pub fn resolve_secret_value(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    if let Some(rest) = trimmed.strip_prefix("$$") {
        return Ok(format!("${rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("$!") {
        return Ok(format!("!{rest}"));
    }

    if let Some(cmd) = trimmed.strip_prefix('!') {
        return execute_command(cmd);
    }

    if trimmed.starts_with('$') {
        return expand_env(trimmed);
    }

    Ok(trimmed.to_string())
}

pub async fn resolve_secret_value_async(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    if let Some(rest) = trimmed.strip_prefix("$$") {
        return Ok(format!("${rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("$!") {
        return Ok(format!("!{rest}"));
    }

    if let Some(cmd) = trimmed.strip_prefix('!') {
        return execute_command_async(cmd).await;
    }

    if trimmed.starts_with('$') {
        return expand_env(trimmed);
    }

    Ok(trimmed.to_string())
}

fn execute_command(cmd: &str) -> Result<String> {
    #[cfg(unix)]
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| AppError::Auth(format!("Failed to execute auth command '{cmd}': {e}")))?;

    #[cfg(not(unix))]
    let output = Command::new("cmd")
        .arg("/C")
        .arg(cmd)
        .output()
        .map_err(|e| AppError::Auth(format!("Failed to execute auth command '{cmd}': {e}")))?;

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

async fn run_raw_cmd_async(cmd: &str) -> Result<std::process::Output> {
    #[cfg(unix)]
    let mut c = tokio::process::Command::new("sh");
    #[cfg(unix)]
    c.arg("-c").arg(cmd);

    #[cfg(not(unix))]
    let mut c = tokio::process::Command::new("cmd");
    #[cfg(not(unix))]
    c.arg("/C").arg(cmd);

    c.output()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to execute auth command '{cmd}': {e}")))
}

async fn execute_command_async(cmd: &str) -> Result<String> {
    let output = run_raw_cmd_async(cmd).await?;
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
}
