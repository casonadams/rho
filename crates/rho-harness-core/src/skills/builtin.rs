use super::types::SkillMetadata;

include!(concat!(env!("OUT_DIR"), "/builtin_skills.rs"));

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
