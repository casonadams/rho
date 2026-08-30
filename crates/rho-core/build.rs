use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("builtin_skills.rs");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace = Path::new(&manifest_dir).ancestors().nth(2).unwrap();
    let prompts_skills = workspace.join("prompts").join("skills");
    let skills_dir = workspace.join("skills");

    println!("cargo:rerun-if-changed=prompts/skills");
    println!("cargo:rerun-if-changed=prompts/tools");
    println!("cargo:rerun-if-changed=prompts/SYSTEM.md");

    let mut generated = String::new();
    generated.push_str("pub struct BuiltinSkill {\n");
    generated.push_str("    pub name: &'static str,\n");
    generated.push_str("    pub description: &'static str,\n");
    generated.push_str("    pub content: &'static str,\n");
    generated.push_str("}\n\n");

    generated.push_str("pub static BUILTIN_SKILLS: &[BuiltinSkill] = &[\n");

    let mut scanned_names = HashSet::new();

    for dir in [prompts_skills, skills_dir] {
        if dir.exists()
            && dir.is_dir()
            && let Ok(entries) = fs::read_dir(&dir)
        {
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if path.is_dir() {
                    let skill_file = path.join("SKILL.md");
                    if skill_file.exists() {
                        emit_skill(&skill_file, &mut generated, &mut scanned_names);
                    }
                } else if path.extension().is_some_and(|e| e == "md") {
                    emit_skill(&path, &mut generated, &mut scanned_names);
                }
            }
        }
    }

    generated.push_str("];\n");
    fs::write(dest_path, generated).unwrap();
}

fn emit_skill(path: &Path, out: &mut String, scanned: &mut HashSet<String>) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();
    let mut description = String::new();

    if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            for line in parts[1].lines() {
                let trimmed = line.trim();
                if let Some(val) = trimmed.strip_prefix("name:") {
                    name = val.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(val) = trimmed.strip_prefix("description:") {
                    description = val.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }

    if description.is_empty() {
        description = content
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with('#') && !line.starts_with("---"))
            .unwrap_or("Built-in agent skill")
            .trim()
            .to_string();
    }

    if scanned.contains(&name) {
        return;
    }
    scanned.insert(name.clone());

    let rel_path = path.display().to_string().replace('\\', "/");
    out.push_str("    BuiltinSkill {\n");
    out.push_str(&format!("        name: \"{}\",\n", name.escape_default()));
    out.push_str(&format!("        description: \"{}\",\n", description.escape_default()));
    out.push_str(&format!("        content: include_str!(\"{rel_path}\"),\n"));
    out.push_str("    },\n");
}
