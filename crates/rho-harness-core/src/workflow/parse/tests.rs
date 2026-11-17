use super::*;

fn no_files(path: &str) -> Result<String, String> {
    Err(format!("no file {path}"))
}

fn parse(text: &str) -> Result<Workflow, String> {
    parse_workflow("sample", text, &no_files)
}

const VALID: &str = r#"
description = "Scan and fix"
isolation = "branch"

[[input]]
name = "profile"
choices = ["review", "security"]

[[step]]
id = "scopes"
builtin = "repo_scopes"
with = { max_scopes = 3 }

[[step]]
id = "scan"
agent = "scout"
over = "scopes.items"
concurrency = 2
prompt = "Scan {{item.paths}} for {{inputs.profile}}"

[[step]]
id = "validate"
agent = "critic"
context = ["scan"]
output = "json"
prompt = "Validate"

[[step]]
id = "implement"
agent = "general"
when = "validate.actionable"
check = "make all"
retries = 2
on_fail = "stash"
prompt = "Fix {{validate.actionable}}"

[outputs]
actionable = "validate.actionable"
excluded = ["implement.wont_fix"]
"#;

#[test]
fn parses_a_valid_workflow_into_typed_steps() {
    let wf = parse(VALID).unwrap();
    assert_eq!(wf.name, "sample");
    assert_eq!(wf.isolation, Isolation::Branch);
    assert_eq!(
        wf.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["scopes", "scan", "validate", "implement"]
    );
    let StepKind::Builtin { builtin, with } = &wf.steps[0].kind else {
        panic!("expected builtin");
    };
    assert_eq!(*builtin, Builtin::RepoScopes);
    assert_eq!(with["max_scopes"], "3");
    assert_eq!(wf.steps[1].concurrency, 2);
    assert_eq!(wf.steps[2].output, OutputFormat::Json);
    assert_eq!(
        wf.steps[3].check,
        Some(Check {
            command: "make all".into(),
            retries: 2,
            on_fail: OnFail::Stash
        })
    );
    assert_eq!(wf.outputs["excluded"], vec!["implement.wont_fix"]);
}

#[test]
fn reports_every_invalid_reference_and_shape_error() {
    let text = r#"
[[step]]
id = "a"
agent = "scout"
prompt = "{{item.x}} {{inputs.missing}} {{later.out}}"

[[step]]
id = "a"
run = "echo"
prompt = "nope"
context = ["zzz"]

[[step]]
id = "round"
agent = "scout"
run = "x"

[outputs]
out = "ghost.value"
"#;
    let err = parse(text).unwrap_err();
    for expected in [
        "unknown reference 'item.x' (only valid in steps with `over`)",
        "unknown reference 'inputs.missing'",
        "unknown reference 'later.out'",
        "duplicate step id",
        "`prompt` and `prompt_file` are only valid on agent steps",
        "context 'zzz' must name an earlier step",
        "invalid step id 'round'",
        "set exactly one of",
        "unknown reference 'ghost.value'",
    ] {
        assert!(err.contains(expected), "missing '{expected}' in:\n{err}");
    }
}

#[test]
fn rejects_misplaced_step_options() {
    let cases = [
        (
            "[[step]]\nid = \"a\"\nrun = \"x\"\ncheck = \"make\"",
            "only supported on agent steps",
        ),
        (
            "[[step]]\nid = \"a\"\nagent = \"g\"\nprompt = \"p\"\nretries = 1",
            "require `check`",
        ),
        (
            "[[step]]\nid = \"a\"\nrun = \"x\"\ncarry = [\"e\"]",
            "only valid on workflow steps",
        ),
        (
            "[[step]]\nid = \"a\"\nworkflow = \"w\"\ncarry = [\"e\"]",
            "`carry` requires `repeat`",
        ),
        (
            "[[step]]\nid = \"a\"\nworkflow = \"w\"\nrepeat = { until_empty = \"x\", max = 0 }",
            "positive integer",
        ),
        ("[[step]]\nid = \"a\"\nbuiltin = \"nope\"", "unknown builtin 'nope'"),
        (
            "[[step]]\nid = \"a\"\nagent = \"g\"",
            "exactly one of `prompt` or `prompt_file`",
        ),
        (
            "[[step]]\nid = \"a\"\nagent = \"g\"\nprompt = \"p\"\noutput = \"xml\"",
            "output must be",
        ),
        (
            "isolation = \"vm\"\n[[step]]\nid = \"a\"\nrun = \"x\"",
            "isolation must be",
        ),
        ("name = \"x\"", "has no steps"),
        ("[[step]]\nid = \"a\"\nrun = \"x\"\nbogus = 1", "invalid workflow TOML"),
    ];
    for (text, expected) in cases {
        let err = parse(text).unwrap_err();
        assert!(err.contains(expected), "missing '{expected}' in:\n{err}");
    }
}

#[test]
fn loads_prompt_files_through_the_reader() {
    let reader = |path: &str| -> Result<String, String> {
        if path == "prompts/scan.md" {
            Ok("Scan {{inputs.profile}}".to_string())
        } else {
            Err("missing".to_string())
        }
    };
    let text =
        "[[input]]\nname = \"profile\"\n[[step]]\nid = \"scan\"\nagent = \"scout\"\nprompt_file = \"prompts/scan.md\"";
    let wf = parse_workflow("w", text, &reader).unwrap();
    let StepKind::Agent(agent) = &wf.steps[0].kind else {
        panic!("expected agent");
    };
    assert_eq!(agent.prompt, "Scan {{inputs.profile}}");

    let missing = "[[step]]\nid = \"scan\"\nagent = \"scout\"\nprompt_file = \"prompts/nope.md\"";
    assert!(
        parse_workflow("w", missing, &reader)
            .unwrap_err()
            .contains("cannot read prompt_file")
    );
}

#[test]
fn repeat_max_accepts_templates_and_numbers() {
    let text = r#"
[[input]]
name = "max_rounds"
default = 6

[[step]]
id = "rounds"
workflow = "round"
repeat = { until_empty = "actionable", max = "{{inputs.max_rounds}}", stop_on_no_progress = true }
carry = ["excluded"]
"#;
    let wf = parse(text).unwrap();
    let StepKind::Workflow(call) = &wf.steps[0].kind else {
        panic!("expected workflow call");
    };
    let repeat = call.repeat.as_ref().unwrap();
    assert_eq!(repeat.max, "{{inputs.max_rounds}}");
    assert!(repeat.stop_on_no_progress);
    assert_eq!(call.carry, vec!["excluded"]);
}
