include!(concat!(env!("OUT_DIR"), "/builtin_skills.rs"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub location: String,
}

pub fn builtin_skills() -> Vec<SkillMetadata> {
    BUILTIN_SKILLS
        .iter()
        .map(|skill| SkillMetadata {
            name: skill.name.to_string(),
            description: skill.description.to_string(),
            location: format!("rho://skills/{}", skill.name),
        })
        .collect()
}

pub fn get_builtin_skill_content(name: &str) -> Option<&'static str> {
    let clean = name
        .trim()
        .trim_start_matches("rho://skills/")
        .trim_end_matches("/SKILL.md")
        .trim_end_matches(".md");
    BUILTIN_SKILLS
        .iter()
        .find(|skill| skill.name == clean)
        .map(|skill| skill.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_skills_embedded_at_build_time() {
        let skills = builtin_skills();
        assert!(skills.iter().any(|s| s.name == "create-plugin"));
        assert!(skills.iter().any(|s| s.name == "plan"));

        let plan_content = get_builtin_skill_content("plan").unwrap();
        assert!(plan_content.contains("Plan Implementation Workflow"));

        let plugin_content = get_builtin_skill_content("rho://skills/create-plugin").unwrap();
        assert!(plugin_content.contains("Creating a Plugin for `rho`"));
    }
}
