use super::types::SkillMetadata;
use std::io::Read;
use std::path::Path;

/// Read a bounded prefix of the file; skills declare their metadata in the
/// leading frontmatter, so full reads are unnecessary while scanning.
const SKILL_METADATA_PREFIX_BYTES: u64 = 4096;
const FALLBACK_DESCRIPTION: &str = "Custom agent skill";

pub fn parse_skill_file(path: &Path) -> Option<SkillMetadata> {
    let content = read_skill_prefix(path)?;
    let declared_name = if path.file_name().is_some_and(|name| name == "SKILL.md") {
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    } else {
        path.file_stem().and_then(|name| name.to_str()).map(str::to_string)
    };
    Some(build_metadata(path, declared_name, &content))
}

fn read_skill_prefix(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let limited = file.take(SKILL_METADATA_PREFIX_BYTES);
    let mut prefix = String::new();
    let mut reader = std::io::BufReader::new(limited);
    reader.read_to_string(&mut prefix).ok()?;
    Some(prefix)
}

fn extract_body_description(content: &str) -> String {
    content
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with('#') && !line.starts_with("---"))
        .unwrap_or(FALLBACK_DESCRIPTION)
        .trim()
        .to_string()
}

fn build_metadata(path: &Path, declared_name: Option<String>, content: &str) -> SkillMetadata {
    let frontmatter = crate::frontmatter::parse_frontmatter(content);
    let name = frontmatter
        .as_ref()
        .and_then(|fm| fm.get("name").map(str::to_string))
        .or(declared_name)
        .unwrap_or_else(|| "skill".to_string());
    let description = frontmatter
        .as_ref()
        .and_then(|fm| fm.get("description").map(str::to_string))
        .unwrap_or_else(|| extract_body_description(content));
    let disable_model_invocation = frontmatter
        .as_ref()
        .and_then(|fm| fm.get_with_aliases(&["disable-model-invocation", "disable_model_invocation"]))
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    SkillMetadata {
        name,
        description,
        location: path.display().to_string(),
        disable_model_invocation,
    }
}
