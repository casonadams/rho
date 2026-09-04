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

#[test]
fn user_skill_resolves_with_user_origin_and_content() {
    let fixture = fixture();
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "plan",
        "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n",
    );
    write_skill(
        &fixture.config_dir.join("skills"),
        "ignored-config",
        "---\nname: ignored-config\ndescription: Ignored config skill\n---\n# Ignored\n",
    );
    write_skill(
        &fixture.home_dir.join(".config/agents/skills"),
        "ignored-xdg",
        "---\nname: ignored-xdg\ndescription: Ignored XDG skill\n---\n# Ignored\n",
    );
    write_skill(
        &fixture.home_dir.join(".skills"),
        "ignored-dot-skills",
        "---\nname: ignored-dot-skills\ndescription: Ignored dot skill\n---\n# Ignored\n",
    );

    let paths = SkillResolutionPaths {
        project_dir: None,
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    assert_eq!(resolved.len(), 1);
    let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
    assert_eq!(plan.origin, SkillOrigin::User);
    assert_eq!(plan.metadata.description, "User plan override");
    assert!(plan.metadata.location.contains(".agents/skills/plan/SKILL.md"));
    assert_eq!(
        std::fs::read_to_string(&plan.metadata.location).unwrap(),
        "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n"
    );
}

#[test]
fn project_override_beats_user_and_user_additions_survive() {
    let fixture = fixture();
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "plan",
        "---\nname: plan\ndescription: User plan\n---\n# User Plan\n",
    );
    write_skill(
        &fixture.project_dir.join(".rho/skills"),
        "plan",
        "---\nname: plan\ndescription: Project plan\n---\n# Project Plan\n",
    );
    write_skill(
        &fixture.home_dir.join(".agents/skills"),
        "team-notes",
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\n",
    );

    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);
    let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
    assert_eq!(plan.origin, SkillOrigin::Project);
    assert_eq!(plan.metadata.description, "Project plan");
    assert!(
        std::fs::read_to_string(&plan.metadata.location)
            .unwrap()
            .contains("# Project Plan")
    );

    let notes = resolved
        .iter()
        .find(|skill| skill.metadata.name == "team-notes")
        .unwrap();
    assert_eq!(notes.origin, SkillOrigin::User);
    assert!(
        std::fs::read_to_string(&notes.metadata.location)
            .unwrap()
            .contains("# Notes")
    );
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

#[test]
fn agents_skills_user_and_project_resolution() {
    let fixture = fixture();
    let user_agents_skills = fixture.home_dir.join(".agents/skills");
    let project_agents_skills = fixture.project_dir.join(".agents/skills");

    write_skill(
        &user_agents_skills,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Global tool\n---\n# Global\n",
    );
    write_skill(
        &project_agents_skills,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Project tool override\n---\n# Project Override\n",
    );
    write_skill(
        &project_agents_skills,
        "repo-lint",
        "---\nname: repo-lint\ndescription: Repo lint workflow\n---\n# Lint\n",
    );

    let paths = SkillResolutionPaths {
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&fixture.home_dir),
    };
    let resolved = resolved_skills_for_paths(paths);

    let shared = resolved.iter().find(|s| s.metadata.name == "shared-tool").unwrap();
    assert_eq!(shared.origin, SkillOrigin::Project);
    assert_eq!(shared.metadata.description, "Project tool override");

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
