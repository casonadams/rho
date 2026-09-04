use rho_harness_core::provider::ProviderId;

use super::types::{BUILTIN_SLASH_COMMANDS, CommandItem, ModelItem, ProviderItem, SkillItem};

#[derive(Debug, Clone)]
pub struct CompletionSet {
    pub(super) commands: Vec<CommandItem>,
    pub(super) skills: Vec<SkillItem>,
    pub(super) models: Vec<ModelItem>,
    pub(super) providers: Vec<ProviderItem>,
    pub(super) files: Vec<String>,
}

impl CompletionSet {
    pub fn from_sources(sources: super::super::sources::CompletionSources) -> Self {
        let mut commands = Vec::new();
        for (name, desc) in BUILTIN_SLASH_COMMANDS {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: (*desc).to_string(),
            });
        }
        for name in &sources.prompt_templates {
            commands.push(CommandItem {
                name: format!("/{name}"),
                description: "Custom prompt template".to_string(),
            });
        }
        // Register each skill directly as a top-level `/skill:<name>` command (Pi-style)
        for s in &sources.skills {
            commands.push(CommandItem {
                name: format!("/skill:{}", s.metadata.name),
                description: format!("{} [{}]", s.metadata.description, s.origin),
            });
        }
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        commands.dedup_by(|a, b| a.name == b.name);

        let skills = sources
            .skills
            .into_iter()
            .map(|s| SkillItem {
                name: s.metadata.name,
                description: s.metadata.description,
                origin: s.origin.to_string(),
            })
            .collect();

        let mut providers = Vec::new();
        for p in ProviderId::ALL {
            providers.push(ProviderItem {
                name: p.as_str().to_string(),
                auth_mode: p.auth_mode_label().to_string(),
            });
        }
        for name in sources.custom_providers {
            if !providers.iter().any(|p| p.name == name) {
                providers.push(ProviderItem {
                    name,
                    auth_mode: "custom endpoint".to_string(),
                });
            }
        }

        let cwd = std::env::current_dir().ok();
        let files = cwd
            .as_deref()
            .map(|d| rho_harness_core::workspace::list_relative_files(d, 2000))
            .unwrap_or_default();

        Self {
            commands,
            skills,
            models: sources.models,
            providers,
            files,
        }
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files = files;
        self
    }
}
