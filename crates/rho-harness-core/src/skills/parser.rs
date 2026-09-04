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
        // Directory skills: `<name>/SKILL.md` is named for the directory.
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    } else {
        // Flat files: `<name>.md` is named for its file stem.
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

fn build_metadata(path: &Path, declared_name: Option<String>, content: &str) -> SkillMetadata {
    let mut name = declared_name.unwrap_or_else(|| "skill".to_string());
    let mut description = String::new();

    if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            for line in parts[1].lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(value) = trimmed.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }

    if description.is_empty() {
        description = content
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with('#') && !line.starts_with("---"))
            .unwrap_or(FALLBACK_DESCRIPTION)
            .trim()
            .to_string();
    }

    SkillMetadata {
        name,
        description,
        location: path.display().to_string(),
    }
}
