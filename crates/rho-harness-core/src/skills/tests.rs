use super::*;
use std::path::{Path, PathBuf};

struct SkillFixture {
    root: PathBuf,
    config_dir: PathBuf,
    project_dir: PathBuf,
    home_dir: PathBuf,
}

impl Drop for SkillFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> SkillFixture {
    let root = std::env::temp_dir().join(format!("skills_{}", uuid::Uuid::new_v4()));
    let config_dir = root.join("config");
    let project_dir = root.join("project");
    let home_dir = root.join("home");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir_all(&home_dir).unwrap();
    SkillFixture {
        root,
        config_dir,
        project_dir,
        home_dir,
    }
}

fn write_skill(dir: &Path, name: &str, body: &str) {
    let skill_dir = dir.join(name);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), body).unwrap();
}

#[test]
fn empty_directories_resolve_to_no_skills() {
    let fixture = fixture();
    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    assert!(resolved.is_empty());
}

fn write_ignored_skills(fixture: &SkillFixture) {
    let ignored = [
        (fixture.config_dir.join("skills"), "ignored-config"),
        (fixture.home_dir.join(".config/agents/skills"), "ignored-xdg"),
        (fixture.home_dir.join(".skills"), "ignored-dot-skills"),
    ];
    for (dir, name) in &ignored {
        let content = format!("---\nname: {name}\ndescription: desc\n---\n# Ignored\n");
        write_skill(dir, name, &content);
    }
}

#[test]
fn user_skill_resolves_with_user_origin_and_content() {
    let fixture = fixture();
    let plan_content = "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n";
    write_skill(&fixture.home_dir.join(".agents/skills"), "plan", plan_content);
    write_ignored_skills(&fixture);

    let paths = SkillResolutionPaths {
        project_dir: None,
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    assert_eq!(resolved.len(), 1);
    let plan = resolved.iter().find(|s| s.metadata.name == "plan").unwrap();
    let actual = (
        plan.origin,
        plan.metadata.description.as_str(),
        plan.metadata.location.contains(".agents/skills/plan/SKILL.md"),
    );
    assert_eq!(actual, (SkillOrigin::User, "User plan override", true));
    assert_eq!(std::fs::read_to_string(&plan.metadata.location).unwrap(), plan_content);
}

fn assert_skill_entry(skill: &ResolvedSkill, origin: SkillOrigin, desc: &str, marker: &str) {
    assert_eq!((skill.origin, skill.metadata.description.as_str()), (origin, desc));
    assert!(
        std::fs::read_to_string(&skill.metadata.location)
            .unwrap()
            .contains(marker)
    );
}

fn seed_override_skills(fixture: &SkillFixture) {
    let home = fixture.home_dir.join(".agents/skills");
    write_skill(
        &home,
        "plan",
        "---\nname: plan\ndescription: User plan\n---\n# User Plan\n",
    );
    write_skill(
        &fixture.project_dir.join(".rho/skills"),
        "plan",
        "---\nname: plan\ndescription: Project plan\n---\n# Project Plan\n",
    );
    write_skill(
        &home,
        "team-notes",
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\n",
    );
}

#[test]
fn project_override_beats_user_and_user_additions_survive() {
    let fixture = fixture();
    seed_override_skills(&fixture);
    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    let plan = resolved.iter().find(|s| s.metadata.name == "plan").unwrap();
    assert_skill_entry(plan, SkillOrigin::Project, "Project plan", "# Project Plan");

    let notes = resolved.iter().find(|s| s.metadata.name == "team-notes").unwrap();
    assert_skill_entry(notes, SkillOrigin::User, "User notes workflow", "# Notes");
}

#[test]
fn flat_skill_files_use_their_file_stem_as_name() {
    let fixture = fixture();
    let skills_dir = fixture.project_dir.join(".rho/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("deploy.md"), "# Deploy workflow\nPush builds.\n").unwrap();

    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    let deploy = resolved
        .iter()
        .find(|skill| skill.metadata.name == "deploy")
        .expect("flat file stem becomes the skill name");
    assert_eq!(deploy.origin, SkillOrigin::Project);
    assert_eq!(deploy.metadata.description, "Push builds.");
    assert!(
        std::fs::read_to_string(&deploy.metadata.location)
            .unwrap()
            .contains("Push builds.")
    );
}

fn seed_agent_skills(user: &Path, proj: &Path) {
    write_skill(
        user,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Global tool\n---\n# Global\n",
    );
    write_skill(
        proj,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Override\n---\n# Override\n",
    );
    write_skill(
        proj,
        "repo-lint",
        "---\nname: repo-lint\ndescription: Lint\n---\n# Lint\n",
    );
}

#[test]
fn agents_skills_user_and_project_resolution() {
    let fixture = fixture();
    seed_agent_skills(
        &fixture.home_dir.join(".agents/skills"),
        &fixture.project_dir.join(".agents/skills"),
    );

    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    let shared = resolved.iter().find(|s| s.metadata.name == "shared-tool").unwrap();
    assert_eq!(
        (shared.origin, shared.metadata.description.as_str()),
        (SkillOrigin::Project, "Override")
    );
    let lint = resolved.iter().find(|s| s.metadata.name == "repo-lint").unwrap();
    assert_eq!(lint.origin, SkillOrigin::Project);
}

#[test]
fn resolved_skills_with_home_respects_explicit_override() {
    let fixture = fixture();
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "custom-workflow",
        "---\nname: custom-workflow\ndescription: Custom workflow\n---\n# Workflow\n",
    );

    let resolved = resolved_skills_with_home(Some(&fixture.project_dir), Some(&fixture.home_dir));
    let skill = resolved.iter().find(|s| s.metadata.name == "custom-workflow").unwrap();
    assert_eq!(skill.origin, SkillOrigin::User);
    assert_eq!(skill.metadata.description, "Custom workflow");
}

#[test]
fn disable_model_invocation_parsed_from_frontmatter() {
    let fixture = fixture();
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "slash-only",
        "---\nname: slash-only\ndescription: Slash command only\ndisable-model-invocation: true\n---\n# Slash only\n",
    );
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "model-enabled",
        "---\nname: model-enabled\ndescription: Model enabled\ndisable_model_invocation: false\n---\n# Model enabled\n",
    );

    let paths = SkillResolutionPaths {
        project_dir: None,
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    let slash_only = resolved.iter().find(|s| s.metadata.name == "slash-only").unwrap();
    assert!(slash_only.metadata.disable_model_invocation);

    let model_enabled = resolved.iter().find(|s| s.metadata.name == "model-enabled").unwrap();
    assert!(!model_enabled.metadata.disable_model_invocation);
}
