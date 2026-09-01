use crate::config::Config;
use crate::ui::TerminalRenderer;
use rho_core::provider::ProviderId;
use std::fmt::Write as _;
use std::str::FromStr;

pub fn print_help(config: &Config, renderer: &TerminalRenderer) {
    let mut output = "\nCommands\n\
  /help                       Show this reference\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /skill [name]               List or inspect skills\n\
  /session                    Display token capacity and session diagnostics\n\
  /compact [instructions]     Summarize earlier context to free context space\n\
  /tree                       View conversation turn and branch tree\n\
  /fork [id]                  Fork session from turn or node into a new session\n\
  /clone                      Duplicate active branch into a new session\n\
  /name [name]                Assign a human-readable name to the session\n\
  /rewind <turn>              Rewind context to a specific prior turn\n\
  /clear                      Start a new session; preserve history\n\
  /login [provider]           Add API-key or subscription auth\n\
  /logout [provider]          Remove stored provider auth\n\
  /exit                       Exit rho\n\
\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Ctrl+C                      Cancel the active operation\n\
  Ctrl+D                      Exit at an empty prompt\n\
\nCurrent session\n"
        .to_string();

    let _ = writeln!(output, "  Model                       {}", config.model);
    if let Ok(provider) = ProviderId::from_str(&config.provider) {
        let _ = writeln!(output, "  Provider                    {provider}");
        let _ = writeln!(output, "  Auth mode                   {}", provider.auth_mode_label());
    } else {
        let _ = writeln!(output, "  Provider                    {}", config.provider);
    }
    let _ = writeln!(output, "  Auto approve                {}", config.auto_approve);
    let _ = writeln!(
        output,
        "  Thinking                    {}",
        config.thinking_level.as_deref().unwrap_or("none")
    );
    renderer.write_output(&output);
}
