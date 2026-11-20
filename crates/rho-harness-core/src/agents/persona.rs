use super::parser::split_document;
use std::path::Path;

/// Behavior instructions layered onto any subagent role or custom agent; tools come from the role or agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaMetadata {
    pub name: String,
    pub description: String,
    pub location: String,
    pub instructions: String,
}

pub fn parse_persona_file(path: &Path) -> Option<PersonaMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let fallback_name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("persona");
    let (frontmatter, instructions) = split_document(&content);
    Some(PersonaMetadata {
        name: frontmatter.name.unwrap_or_else(|| fallback_name.to_string()),
        description: frontmatter.description.unwrap_or_default(),
        location: path.display().to_string(),
        instructions,
    })
}
