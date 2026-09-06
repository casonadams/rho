use super::super::LiveIo;
use super::super::bash_runner::run_user_bash;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::TerminalBackend;

async fn run_discarding_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let _ = run_user_bash(cmd, renderer, io).await;
    Ok(None)
}

async fn run_prompt_with_output<B: TerminalBackend>(
    cmd: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    let res = run_user_bash(cmd, renderer, io).await?;
    if res.is_cancelled {
        return Ok(None);
    }
    let status = if res.is_error { " (failed)" } else { "" };
    Ok(Some(format!(
        "Executed local shell command: `{cmd}`{status}\n\nOutput:\n```\n{}\n```",
        res.output
    )))
}

pub(super) async fn resolve_effective_prompt<B: TerminalBackend>(
    input: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    if let Some(cmd) = input.strip_prefix("!!").map(str::trim).filter(|c| !c.is_empty()) {
        return run_discarding_output(cmd, renderer, io).await;
    }
    if let Some(cmd) = input.strip_prefix('!').map(str::trim).filter(|c| !c.is_empty()) {
        return run_prompt_with_output(cmd, renderer, io).await;
    }
    Ok(Some(input.to_string()))
}
