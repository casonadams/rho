use super::*;
use std::path::{Path, PathBuf};

struct SkillFixture {
    root: PathBuf,
    config_dir: PathBuf,
    project_dir: PathBuf,
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
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    SkillFixture {
        root,
        config_dir,
        project_dir,
    }
}

fn write_builtin_override(dir: &Path, name: &str, body: &str) {
    let skill_dir = dir.join(name);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), body).unwrap();
}

#[test]
fn builtins_resolve_with_builtin_origin_and_content() {
    let resolved = resolved_skills(None, None, true);
    let plan = resolved
        .iter()
        .find(|skill| skill.metadata.name == "plan")
        .expect("plan is an embedded built-in");
    assert_eq!(plan.origin, SkillOrigin::Builtin);
    assert_eq!(
        resolved_content(&resolved, "plan").unwrap(),
        get_builtin_skill_content("plan").unwrap()
    );
}

#[test]
fn user_override_replaces_same_name_builtin() {
    let fixture = fixture();
    write_builtin_override(
        &fixture.config_dir.join("skills"),
        "plan",
        "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n",
    );

    let resolved = resolved_skills(Some(&fixture.config_dir), None, true);
    let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
    assert_eq!(plan.origin, SkillOrigin::User);
    assert_eq!(plan.metadata.description, "User plan override");
    assert!(plan.metadata.location.contains("config/skills/plan/SKILL.md"));
    assert_eq!(
        resolved_content(&resolved, "plan").unwrap(),
        "---\nname: plan\ndescription: User plan override\n---\n# Custom Plan\n"
    );
}

#[test]
fn project_override_beats_user_and_user_additions_survive() {
    let fixture = fixture();
    write_builtin_override(
        &fixture.config_dir.join("skills"),
        "plan",
        "---\nname: plan\ndescription: User plan\n---\n# User Plan\n",
    );
    write_builtin_override(
        &fixture.project_dir.join(".rho/skills"),
        "plan",
        "---\nname: plan\ndescription: Project plan\n---\n# Project Plan\n",
    );
    write_builtin_override(
        &fixture.config_dir.join("skills"),
        "team-notes",
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\n",
    );

    let resolved = resolved_skills(Some(&fixture.config_dir), Some(&fixture.project_dir), true);
    let plan = resolved.iter().find(|skill| skill.metadata.name == "plan").unwrap();
    assert_eq!(plan.origin, SkillOrigin::Project);
    assert_eq!(plan.metadata.description, "Project plan");
    assert!(resolved_content(&resolved, "plan").unwrap().contains("# Project Plan"));

    let notes = resolved
        .iter()
        .find(|skill| skill.metadata.name == "team-notes")
        .unwrap();
    assert_eq!(notes.origin, SkillOrigin::User);
    assert!(resolved_content(&resolved, "team-notes").unwrap().contains("# Notes"));
}

#[test]
fn flat_skill_files_use_their_file_stem_as_name() {
    let fixture = fixture();
    let skills_dir = fixture.project_dir.join(".rho/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("deploy.md"), "# Deploy workflow\nPush builds.\n").unwrap();

    let resolved = resolved_skills(None, Some(&fixture.project_dir), true);
    let deploy = resolved
        .iter()
        .find(|skill| skill.metadata.name == "deploy")
        .expect("flat file stem becomes the skill name");
    assert_eq!(deploy.origin, SkillOrigin::Project);
    assert_eq!(deploy.metadata.description, "Push builds.");
    assert!(resolved_content(&resolved, "deploy").unwrap().contains("Push builds."));
}

#[test]
fn disable_builtins_excludes_embedded_but_keeps_user_skills() {
    let fixture = fixture();
    write_builtin_override(
        &fixture.config_dir.join("skills"),
        "team-notes",
        "---\nname: team-notes\ndescription: User notes workflow\n---\n# Notes\n",
    );

    let resolved = resolved_skills(Some(&fixture.config_dir), Some(&fixture.project_dir), false);
    assert!(resolved.iter().all(|skill| skill.origin != SkillOrigin::Builtin));
    assert!(resolved.iter().any(|skill| skill.metadata.name == "team-notes"));
    assert!(resolved_content(&resolved, "plan").is_none());
}

#[test]
fn agents_skills_user_and_project_resolution() {
    let fixture = fixture();
    let home = fixture.root.join("home");
    let user_agents_skills = home.join(".agents/skills");
    let project_agents_skills = fixture.project_dir.join(".agents/skills");

    write_builtin_override(
        &user_agents_skills,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Global tool\n---\n# Global\n",
    );
    write_builtin_override(
        &project_agents_skills,
        "shared-tool",
        "---\nname: shared-tool\ndescription: Project tool override\n---\n# Project Override\n",
    );
    write_builtin_override(
        &project_agents_skills,
        "repo-lint",
        "---\nname: repo-lint\ndescription: Repo lint workflow\n---\n# Lint\n",
    );

    let paths = SkillResolutionPaths {
        config_dir: Some(&fixture.config_dir),
        project_dir: Some(&fixture.project_dir),
        home_dir: Some(&home),
    };
    let resolved = resolved_skills_for_paths(paths, true);

    let shared = resolved.iter().find(|s| s.metadata.name == "shared-tool").unwrap();
    assert_eq!(shared.origin, SkillOrigin::Project);
    assert_eq!(shared.metadata.description, "Project tool override");

    let lint = resolved.iter().find(|s| s.metadata.name == "repo-lint").unwrap();
    assert_eq!(lint.origin, SkillOrigin::Project);
}
