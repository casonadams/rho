use super::types::SkillMetadata;
use std::io::Read;
use std::path::Path;

/// Read a bounded prefix of the file; skills declare their metadata in the
/// leading frontmatter, so full reads are unnecessary while scanning.
const SKILL_METADATA_PREFIX_BYTES: u64 = 4096;
const FALLBACK_DESCRIPTION: &str = "Custom agent skill";

#[derive(Default)]
struct ParsedFrontmatter {
    name: Option<String>,
    description: Option<String>,
    disable_model_invocation: bool,
}

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

fn parse_frontmatter_line(line: &str, out: &mut ParsedFrontmatter) {
    if let Some(value) = line.strip_prefix("name:") {
        out.name = Some(value.trim().trim_matches('"').trim_matches('\'').to_string());
    } else if let Some(value) = line.strip_prefix("description:") {
        out.description = Some(value.trim().trim_matches('"').trim_matches('\'').to_string());
    } else if let Some(value) = line
        .strip_prefix("disable-model-invocation:")
        .or_else(|| line.strip_prefix("disable_model_invocation:"))
    {
        out.disable_model_invocation = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .eq_ignore_ascii_case("true");
    }
}

fn parse_skill_frontmatter(content: &str) -> ParsedFrontmatter {
    let mut out = ParsedFrontmatter::default();
    if !content.starts_with("---") {
        return out;
    }
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() >= 3 {
        for line in parts[1].lines() {
            parse_frontmatter_line(line.trim(), &mut out);
        }
    }
    out
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
    let frontmatter = parse_skill_frontmatter(content);
    let name = frontmatter
        .name
        .or(declared_name)
        .unwrap_or_else(|| "skill".to_string());
    let description = frontmatter
        .description
        .unwrap_or_else(|| extract_body_description(content));
    SkillMetadata {
        name,
        description,
        location: path.display().to_string(),
        disable_model_invocation: frontmatter.disable_model_invocation,
    }
}
