use super::super::LiveIo;
use super::super::bash_runner::run_user_bash;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::TerminalBackend;

pub(super) async fn resolve_effective_prompt<B: TerminalBackend>(
    input: &str,
    renderer: &TerminalRenderer,
    io: &mut LiveIo<'_, B>,
) -> Result<Option<String>> {
    if let Some(cmd) = input.strip_prefix("!!") {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            let _ = run_user_bash(cmd, renderer, io).await;
            return Ok(None);
        }
    }

    if let Some(cmd) = input.strip_prefix('!') {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            let res = run_user_bash(cmd, renderer, io).await?;
            if res.is_cancelled {
                return Ok(None);
            }
            let text = format!(
                "Executed local shell command: `{cmd}`{}\n\nOutput:\n```\n{}\n```",
                if res.is_error { " (failed)" } else { "" },
                res.output
            );
            return Ok(Some(text));
        }
    }

    Ok(Some(input.to_string()))
}
