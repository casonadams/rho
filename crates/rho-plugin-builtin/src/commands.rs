use async_trait::async_trait;
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse};

pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("help", "Print help summary and available slash commands"),
    ("clear", "Reset conversation context and session history"),
    ("model", "Show or switch AI model and provider"),
    ("skill", "List resolved skills or print skill details"),
    ("plugin", "Inspect or manage installed capability plugins"),
    ("login", "Authenticate with an AI provider"),
    ("logout", "Log out from an AI provider and clear credentials"),
    ("exit", "Exit the interactive REPL session"),
];

#[derive(Debug, Clone)]
pub struct BuiltinCommand {
    pub name: &'static str,
    pub description: &'static str,
}

impl BuiltinCommand {
    pub fn all() -> Vec<Self> {
        BUILTIN_COMMANDS
            .iter()
            .map(|(name, desc)| Self {
                name,
                description: desc,
            })
            .collect()
    }
}

#[async_trait]
impl CommandCapability for BuiltinCommand {
    fn descriptor(&self) -> CommandDescriptor {
        CommandDescriptor {
            id: format!("command:{}", self.name).parse().unwrap(),
            name: self.name.to_string(),
            description: self.description.to_string(),
        }
    }

    async fn invoke(&self, _request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError> {
        Ok(CommandInvocationResponse {
            output: format!("/{}: {}", self.name, self.description),
            exit_code: 0,
        })
    }
}
