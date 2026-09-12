use crate::config::Config;
use crate::ui::TerminalRenderer;
use rho_harness_core::provider::ProviderId;
use std::fmt::Write as _;
use std::str::FromStr;

const HELP_REFERENCE: &str = "\nCommands\n\
  /help                       Show this reference\n\
  /settings                   Interactive runtime interface settings\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /resume [id]                Resume a prior session\n\
  /thinking [level]           Configure thinking effort (off, minimal, low, medium, high, max)\n\
  /skill [name]               List or inspect skills\n\
  /mcp                        List configured MCP servers\n\
  /session                    Display token capacity and session diagnostics\n\
  /compact [instructions]     Summarize earlier context to free context space\n\
  /tree                       View conversation turn and branch tree\n\
  /fork [id]                  Fork session from turn or node into a new session\n\
  /clone                      Duplicate active branch into a new session\n\
  /name [name]                Assign a human-readable name to the session\n\
  /rewind <turn>              Rewind context to a specific prior turn\n\
  /clear                      Start a new session; preserve history (alias: /new)\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /reload                     Re-read config, skills, and MCP tools; keep history\n\
  /export [html|md] [path]    Export the active branch as a readable artifact\n\
  /exit                       Exit rho (alias: /quit)\n\
\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Shift+Tab                   Cycle thinking level\n\
  Escape                      Cancel active execution / operation\n\
  Ctrl+C                      Clear the input prompt\n\
  Ctrl+D                      Exit at an empty prompt\n\
  Ctrl+L                      Select model\n\
  Ctrl+O                      Expand or collapse tool output\n\
  Ctrl+T                      Toggle thinking blocks visibility\n\
\nCurrent session\n";

fn append_session_help(output: &mut String, config: &Config) {
    let _ = writeln!(output, "  Model                       {}", config.model);
    if let Ok(provider) = ProviderId::from_str(&config.provider) {
        let _ = writeln!(output, "  Provider                    {provider}");
        let _ = writeln!(output, "  Auth mode                   {}", provider.auth_mode_label());
    } else {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
    }
    let thinking = config.thinking_level.as_deref().unwrap_or("none");
    let _ = writeln!(output, "  Thinking                    {thinking}");
}

pub fn print_help(config: &Config, renderer: &TerminalRenderer) {
    let mut output = HELP_REFERENCE.to_string();
    append_session_help(&mut output, config);
    renderer.write_output(&output);
}
