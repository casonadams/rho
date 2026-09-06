use crate::error::Result;
use crate::repl::input_reader::TerminalInputReader;
use crate::ui::interactive::{TerminalBackend, TerminalController};

fn resolve_editor_command() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string())
}

async fn apply_edited_text<B: TerminalBackend>(controller: &mut TerminalController<B>, temp_file: &std::path::Path) {
    if let Ok(edited_text) = tokio::fs::read_to_string(temp_file).await {
        controller.state_mut().editor_mut().set_text(edited_text.trim_end());
    }
}

pub(super) async fn open_external_editor<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    input: &mut TerminalInputReader,
) -> Result<()> {
    let current_text = controller.state().editor().text().to_string();
    let temp_file = std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
    let _ = tokio::fs::write(&temp_file, &current_text).await;
    let editor = resolve_editor_command();
    let paused = input.pause()?;
    controller.suspend()?;
    let _status = tokio::process::Command::new(&editor).arg(&temp_file).status().await;
    let controller_res = controller.resume();
    let input_res = paused.resume();
    controller_res?;
    input_res?;
    apply_edited_text(controller, &temp_file).await;
    let _ = tokio::fs::remove_file(temp_file).await;
    Ok(())
}
