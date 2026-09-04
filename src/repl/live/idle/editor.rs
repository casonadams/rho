use crate::error::Result;
use crate::repl::input_reader::TerminalInputReader;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) fn open_external_editor<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    input: &mut TerminalInputReader,
) -> Result<()> {
    let current_text = controller.state().editor().text().to_string();
    let temp_file = std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
    let _ = std::fs::write(&temp_file, &current_text);
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string());
    let paused = input.pause()?;
    controller.suspend()?;
    let status = std::process::Command::new(&editor).arg(&temp_file).status();
    let controller_res = controller.resume();
    let input_res = paused.resume();
    controller_res?;
    input_res?;
    if status.is_ok()
        && let Ok(edited_text) = std::fs::read_to_string(&temp_file)
    {
        controller.state_mut().editor_mut().set_text(edited_text.trim_end());
    }
    let _ = std::fs::remove_file(temp_file);
    Ok(())
}
