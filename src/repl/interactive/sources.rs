use rho_core::skills::ResolvedSkill;

use super::ModelItem;

#[derive(Debug, Clone, Default)]
pub struct CompletionSources {
    pub skills: Vec<ResolvedSkill>,
    pub prompt_templates: Vec<String>,
    pub models: Vec<ModelItem>,
    pub custom_providers: Vec<String>,
}

impl CompletionSources {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skills(mut self, skills: Vec<ResolvedSkill>) -> Self {
        self.skills = skills;
        self
    }

    pub fn with_templates(mut self, templates: Vec<String>) -> Self {
        self.prompt_templates = templates;
        self
    }

    pub fn with_models(mut self, models: Vec<ModelItem>) -> Self {
        self.models = models;
        self
    }

    pub fn with_custom_providers(mut self, providers: Vec<String>) -> Self {
        self.custom_providers = providers;
        self
    }
}
