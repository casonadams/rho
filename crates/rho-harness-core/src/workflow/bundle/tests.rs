use super::*;
use crate::workflow::types::StepKind;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rho_workflow_{label}_{}", uuid::Uuid::new_v4()))
}

#[test]
fn builtin_refine_bundle_loads_with_local_agents_and_round() {
    let catalog = WorkflowCatalog::discover(None, None);
    assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
    let refine = catalog.find("refine").expect("builtin refine");
    assert_eq!(refine.origin, WorkflowOrigin::Builtin);
    assert_eq!(refine.location, "builtin:refine");
    assert!(refine.workflows.contains_key("round"));
    let agents: Vec<&str> = refine.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(agents, ["implementor", "validator"]);
    let validator = refine.agents.iter().find(|a| a.name == "validator").unwrap();
    assert_eq!(
        validator.tools.as_deref(),
        Some(&["read".to_string(), "fd".into(), "rg".into()][..])
    );
    assert_eq!(
        refine.entry.argument_hint(),
        "<review|security|performance|reliability|architecture> [max_rounds] [verify]"
    );
}

#[test]
fn project_workflows_override_builtins_and_single_files_load() {
    let root = temp_root("override");
    let project = root.join("project");
    write(
        &project.join(".agents/workflows/refine/workflow.toml"),
        "description = \"custom\"\n[[step]]\nid = \"a\"\nrun = \"true\"",
    );
    write(
        &project.join(".agents/workflows/hello.toml"),
        "[[step]]\nid = \"greet\"\nagent = \"general\"\nprompt_file = \"hello.md\"",
    );
    write(&project.join(".agents/workflows/hello.md"), "Say hi");

    let catalog = WorkflowCatalog::discover(Some(&project), None);
    assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
    let refine = catalog.find("refine").unwrap();
    assert_eq!(refine.origin, WorkflowOrigin::Project);
    assert_eq!(refine.entry.description, "custom");
    let hello = catalog.find("hello").unwrap();
    let StepKind::Agent(step) = &hello.entry.steps[0].kind else {
        panic!("expected agent step");
    };
    assert_eq!(step.prompt, "Say hi");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn invalid_bundles_are_reported_not_loaded() {
    let root = temp_root("invalid");
    write(&root.join(".agents/workflows/broken.toml"), "[[step]]\nid = \"a\"");
    write(
        &root.join(".agents/workflows/caller/workflow.toml"),
        "[[step]]\nid = \"a\"\nworkflow = \"ghost\"",
    );
    write(
        &root.join(".agents/workflows/shape/workflow.toml"),
        "[[step]]\nid = \"a\"\nworkflow = \"inner\"\nwith = { nope = \"x\" }\nrepeat = { until_empty = \"missing\", max = 2 }\ncarry = [\"acc\"]",
    );
    write(
        &root.join(".agents/workflows/shape/inner.toml"),
        "[[step]]\nid = \"b\"\nrun = \"true\"\n[outputs]\nacc = \"b\"",
    );

    let catalog = WorkflowCatalog::discover(Some(&root), None);
    for name in ["broken", "caller", "shape"] {
        assert!(catalog.find(name).is_none(), "{name} should not load");
    }
    let errors = catalog.errors.join("\n");
    for expected in [
        "set exactly one of",
        "unknown workflow 'ghost'",
        "has no input 'nope'",
        "repeat.until_empty 'missing'",
        "carry 'acc' must be both an input and an output",
    ] {
        assert!(errors.contains(expected), "missing '{expected}' in:\n{errors}");
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn prompt_files_cannot_escape_the_bundle() {
    let source = BundleSource::Embedded(REFINE_FILES);
    for path in ["../secret", "/etc/passwd", ""] {
        assert!(source.read(path).is_err(), "{path}");
    }
    assert!(source.read("prompts/scan.md").is_ok());
}
