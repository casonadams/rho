use crate::config::Config;
use crate::engine::provider::ProviderId;
use crate::ui::TerminalRenderer;
use rho_sdk::contract::CommandCapability;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::str::FromStr;
use std::sync::Arc;

pub fn print_help(
    config: &Config,
    renderer: &TerminalRenderer,
    commands: Option<&BTreeMap<String, Arc<dyn CommandCapability>>>,
) {
    let mut output = "\nCommands\n\
  /help                       Show this reference\n\
  /model [model] [provider]   Inspect or switch the model\n\
  /skill [name]               List or inspect skills\n\
  /plugin                     List discovered plugins\n\
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
  /exit                       Exit rho\n"
        .to_string();

    if let Some(commands) = commands
        && !commands.is_empty()
    {
        output.push_str("\nInstalled Plugin Commands\n");
        for (name, cmd) in commands {
            let desc = cmd.descriptor().description;
            let _ = writeln!(output, "  /{:<26} {}", name, desc);
        }
    }

    output.push_str(
        "\nShortcuts\n\
  Tab                         Complete slash commands & skill names\n\
  Ctrl+C                      Cancel the active operation\n\
  Ctrl+D                      Exit at an empty prompt\n\
\nCurrent session\n",
    );
    let _ = writeln!(output, "  Model                       {}", config.model);
    match ProviderId::from_str(&config.provider) {
        Ok(provider) => {
            let _ = writeln!(
                output,
                "  Provider                    {provider} ({})",
                provider.auth_mode_label()
            );
        }
        Err(_) => {
            let _ = writeln!(
                output,
                "  Provider                    {} (unsupported)",
                config.provider
            );
        }
    }
    let approval = if config.auto_approve {
        "auto-approved"
    } else {
        "confirmation required"
    };
    let _ = writeln!(output, "  Changes                     {approval}\n");
    renderer.print_notice(&output);
}
