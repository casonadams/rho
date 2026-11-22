use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn temp_root(label: &str) -> rho_test_support::TestDir {
    rho_test_support::TestDir::with_prefix(&format!("rho_workflow_{label}_"))
}

#[test]
fn ships_no_builtin_workflows() {
    let catalog = WorkflowCatalog::discover(None, None);
    assert!(catalog.bundles.is_empty());
    assert!(catalog.errors.is_empty());
}

#[test]
fn loads_bundles_and_single_files_with_project_overriding_user() {
    let root = temp_root("discover");
    let (home, project) = (root.join("home"), root.join("project"));
    write(
        &home.join(".agents/workflows/refine.xml"),
        "<workflow description=\"user\"><run cmd=\"true\"/></workflow>",
    );
    write(
        &project.join(".agents/workflows/refine/workflow.xml"),
        "<workflow description=\"project\"><call workflow=\"round\" profile=\"x\"/></workflow>",
    );
    write(
        &project.join(".agents/workflows/refine/round.xml"),
        "<workflow><input name=\"profile\"/><agent use=\"implementor\" prompt-file=\"prompts/fix.md\"/></workflow>",
    );
    write(
        &project.join(".agents/workflows/refine/prompts/fix.md"),
        "Fix {{profile}}",
    );
    write(
        &project.join(".agents/workflows/refine/agents/implementor.md"),
        "---\nname: implementor\ntools: read, edit\n---\nFix things.",
    );

    let catalog = WorkflowCatalog::discover(Some(&project), Some(&home));
    assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
    let refine = catalog.find("refine").unwrap();
    assert_eq!(refine.origin, WorkflowOrigin::Project);
    assert_eq!(refine.entry.description, "project");
    assert!(refine.workflows.contains_key("round"));
    assert_eq!(
        refine.agents[0].tools.as_deref(),
        Some(&["read".to_string(), "edit".into()][..])
    );
    let (owner, round) = catalog.resolve_call(refine, "round").unwrap();
    assert_eq!(owner.name, "refine");
    assert_eq!(round.inputs[0].name, "profile");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn invalid_workflows_are_reported_not_loaded() {
    let root = temp_root("invalid");
    let dir = root.join(".agents/workflows");
    write(&dir.join("broken.xml"), "<workflow><run/></workflow>");
    write(
        &dir.join("caller.xml"),
        "<workflow><call workflow=\"ghost\"/></workflow>",
    );
    write(
        &dir.join("selfish.xml"),
        "<workflow><call workflow=\"selfish\"/></workflow>",
    );
    write(
        &dir.join("shape.xml"),
        "<workflow><call workflow=\"inner\" nope=\"x\"/></workflow>",
    );
    write(
        &dir.join("inner.xml"),
        "<workflow><input name=\"a\"/><run cmd=\"true\"/></workflow>",
    );
    write(&dir.join("notes.txt"), "ignored");

    let catalog = WorkflowCatalog::discover(Some(&root), None);
    let names: Vec<&str> = catalog.bundles.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["inner"]);
    let errors = catalog.errors.join("\n");
    for expected in [
        "missing required attribute 'cmd'",
        "unknown workflow 'ghost'",
        "a workflow cannot call itself",
        "'inner' has no input 'nope'",
    ] {
        assert!(errors.contains(expected), "missing '{expected}' in:\n{errors}");
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn prompt_files_cannot_escape_the_workflow_directory() {
    let root = temp_root("escape");
    for path in ["../secret", "/etc/passwd", ""] {
        assert!(read_relative(&root, path).is_err(), "{path}");
    }
}
