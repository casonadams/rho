use async_trait::async_trait;
use rho_core::skills::builtin_skills;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{SkillAsset, SkillCapability};

#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinSkillCapability;

#[async_trait]
impl SkillCapability for BuiltinSkillCapability {
    fn id(&self) -> CapabilityId {
        "skill:builtin".parse().unwrap()
    }

    async fn assets(&self) -> Result<Vec<SkillAsset>, CapabilityError> {
        let skills = builtin_skills();
        let assets = skills
            .into_iter()
            .map(|meta| SkillAsset {
                id: format!("skill:{}", meta.name).parse().unwrap(),
                name: meta.name.clone(),
                description: meta.description.clone(),
                markdown: rho_core::skills::resolved_content(
                    &rho_core::skills::resolved_skills(None, None),
                    &meta.name,
                )
                .unwrap_or_default(),
            })
            .collect();
        Ok(assets)
    }
}
