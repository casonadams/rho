use crate::ui::TerminalRenderer;
use crossterm::QueueableCommand;
use crossterm::cursor::{MoveToColumn, MoveUp};
use crossterm::terminal::{Clear, ClearType};
use crossterm::tty::IsTty;
use std::io::Write;
use unicode_width::UnicodeWidthStr;

pub enum ShellAction {
    Handled,
    Prompt(String),
    Passthrough,
}

pub fn submitted_input_rows(input: &str, terminal_width: usize) -> u16 {
    let width = terminal_width.max(1);
    input.lines().fold(0_u16, |rows, line| {
        let occupied = UnicodeWidthStr::width(line).saturating_add(2);
        rows.saturating_add((occupied / width + 1).try_into().unwrap_or(u16::MAX))
    })
}

pub fn clear_submitted_input(input: &str) {
    let mut stdout = std::io::stdout();
    if !stdout.is_tty() {
        return;
    }
    let width = crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns))
        .unwrap_or(80);
    let rows = submitted_input_rows(input, width);
    let _ = stdout
        .queue(MoveUp(rows))
        .and_then(|stream| stream.queue(MoveToColumn(0)))
        .and_then(|stream| stream.queue(Clear(ClearType::FromCursorDown)))
        .and_then(Write::flush);
}

pub async fn handle_shell_command(input: &str, renderer: &TerminalRenderer) -> ShellAction {
    if let Some(cmd) = input.strip_prefix("!!") {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            execute_silent_shell(cmd, renderer).await;
            return ShellAction::Handled;
        }
    }

    if let Some(cmd) = input.strip_prefix('!') {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            let prompt = execute_turn_shell(cmd, renderer).await;
            return ShellAction::Prompt(prompt);
        }
    }

    ShellAction::Passthrough
}

async fn run_local_command(cmd: &str) -> std::io::Result<std::process::Output> {
    #[cfg(unix)]
    {
        tokio::process::Command::new("sh").arg("-c").arg(cmd).output().await
    }
    #[cfg(windows)]
    {
        tokio::process::Command::new("cmd.exe")
            .arg("/c")
            .arg(cmd)
            .output()
            .await
    }
}

async fn execute_silent_shell(cmd: &str, renderer: &TerminalRenderer) {
    renderer.print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
    match run_local_command(cmd).await {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout);
            let stderr = String::from_utf8_lossy(&res.stderr);
            if !stdout.is_empty() {
                renderer.write_output(&stdout);
            }
            if !stderr.is_empty() {
                renderer.write_output(&stderr);
            }
        }
        Err(e) => {
            renderer.print_notice(&format!("  Command execution failed: {e}\n"));
        }
    }
}

async fn execute_turn_shell(cmd: &str, renderer: &TerminalRenderer) -> String {
    renderer.print_notice(&format!("  [Executing local shell: `{cmd}`]\n"));
    match run_local_command(cmd).await {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout);
            let stderr = String::from_utf8_lossy(&res.stderr);
            if !stdout.is_empty() {
                renderer.write_output(&stdout);
            }
            if !stderr.is_empty() {
                renderer.write_output(&stderr);
            }
            format!(
                "Executed local shell command: `{cmd}`\n\nOutput:\n```\n{}{}\n```",
                stdout, stderr
            )
        }
        Err(e) => {
            renderer.print_notice(&format!("  Command execution failed: {e}\n"));
            format!("Failed to execute local shell command `{cmd}`: {e}")
        }
    }
}
