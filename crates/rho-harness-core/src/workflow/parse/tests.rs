use super::*;
use crate::workflow::types::Items;

fn no_files(path: &str) -> Result<String, String> {
    Err(format!("no file {path}"))
}

fn parse(text: &str) -> Result<Workflow, String> {
    parse_workflow("sample", text, &no_files)
}

fn wrap(body: &str) -> String {
    format!("<workflow>\n<input name=\"profile\"/>\n{body}\n</workflow>")
}

const REFINE: &str = r#"
<workflow name="refine" description="Scan and fix">
  <input name="profile" choices="review security"/>
  <input name="rounds" default="6"/>

  <loop id="round" max="{{rounds}}" while="{{validate.actionable}}" stop-if-unchanged="{{validate.actionable}}">
    <each id="scan" items="src crates" as="slice" parallel="2">
      <agent use="scout" turns="40">
        Scan {{slice}} for {{profile}}.
          Keep Vec<String> && <file> verbatim.
      </agent>
    </each>
    <agent id="validate" use="validator" model="slow" persona="skeptic" output="json" context="scan">Validate.</agent>
    <loop id="attempt" max="3" until="{{verify.ok}}" when="{{validate.actionable}}">
      <agent id="implement" use="implementor">Fix {{validate.actionable}}; last failure: {{verify.output}}</agent>
      <run id="verify" cmd="make all"/>
    </loop>
    <run when="{{verify.ok}}" cmd="git commit -m 'fix({{profile}}): round {{round.index}}'"/>
  </loop>
  <output name="remaining" value="{{validate.actionable}}"/>
</workflow>
"#;

#[test]
fn parses_a_nested_workflow_into_typed_nodes() {
    let wf = parse(REFINE).unwrap();
    assert_eq!(wf.name, "refine");
    assert_eq!(wf.inputs[1].default.as_deref(), Some("6"));
    assert_eq!(
        wf.outputs,
        vec![("remaining".to_string(), "{{validate.actionable}}".to_string())]
    );
    let NodeKind::Loop(round) = &wf.body[0].kind else {
        panic!("expected loop");
    };
    assert_eq!(round.max, "{{rounds}}");
    let NodeKind::Each(scan) = &round.body[0].kind else {
        panic!("expected each");
    };
    assert_eq!(scan.items, Items::Literal(vec!["src".into(), "crates".into()]));
    assert_eq!((scan.var.as_str(), scan.parallel), ("slice", 2));
    let NodeKind::Agent(scout) = &scan.body[0].kind else {
        panic!("expected agent");
    };
    assert_eq!(
        scout.prompt,
        "Scan {{slice}} for {{profile}}.\n  Keep Vec<String> && <file> verbatim."
    );
    assert_eq!(
        (scout.model.as_deref(), scout.turns, scout.persona.as_deref()),
        (None, 40, None)
    );
    let NodeKind::Agent(validate) = &round.body[1].kind else {
        panic!("expected agent");
    };
    assert_eq!((validate.model.as_deref(), validate.turns), (Some("slow"), 100));
    assert_eq!(validate.persona.as_deref(), Some("skeptic"));
    assert_eq!(validate.output, OutputFormat::Json);
    let NodeKind::Run(commit) = &round.body[3].kind else {
        panic!("expected run");
    };
    assert_eq!(
        commit.args,
        ["git", "commit", "-m", "fix({{profile}}): round {{round.index}}"]
    );
    assert_eq!(round.body[3].when.as_deref(), Some("{{verify.ok}}"));
}

#[test]
fn reports_every_shape_and_reference_problem() {
    let text = wrap(
        r#"<agent use="scout" model="guard" turns="0" bogus="1">Use {{nope}} and {{slice}}</agent>
<run id="b" cmd="echo 'unterminated"/>
<run id="a" cmd="echo"/>
<run id="a" cmd="echo"/>
<run id="profile" cmd="echo"/>
<agent use="x" context="ghost">p</agent>
<agent use="x"/>
<each items="{{a}}" as="profile"><run cmd="echo"/></each>
<loop max="zero"><run cmd="echo"/></loop>
<choose on="{{a}}"><run cmd="echo"/></choose>
<parallel/>
<teleport/>
<output name="o" value="{{ghost.value}}"/>"#,
    );
    let err = parse(&text).unwrap_err();
    for expected in [
        "model must be one of default, smol, slow (got 'guard')",
        "'turns' must be a positive integer",
        "unknown attribute 'bogus'",
        "unknown reference '{{nope}}'",
        "unknown reference '{{slice}}'",
        "unterminated quote",
        "duplicate id 'a'",
        "id 'profile' is already an input name",
        "context 'ghost' is not a step id",
        "needs a prompt",
        "'as' name 'profile' is invalid or already used",
        "'max' must be a positive integer or a reference",
        "only <case> and one <otherwise>",
        "<parallel>: needs at least one step",
        "unknown element 'teleport'",
        "output 'o': unknown reference '{{ghost.value}}'",
    ] {
        assert!(err.contains(expected), "missing '{expected}' in:\n{err}");
    }
}

#[test]
fn each_variable_is_only_visible_inside_its_body() {
    let text = wrap(r#"<each items="a b" as="x"><run cmd="echo {{x}}"/></each><run cmd="echo {{x}}"/>"#);
    let err = parse(&text).unwrap_err();
    assert_eq!(err.matches("unknown reference '{{x}}'").count(), 1, "{err}");
}

#[test]
fn rejects_bad_documents_and_roots() {
    let cases = [
        ("<steps/>", "root element must be <workflow>"),
        ("<workflow></workflow>", "has no steps"),
        (
            "<workflow isolation=\"branch\"><run cmd=\"x\"/></workflow>",
            "every workflow now runs in its own git worktree",
        ),
        ("<workflow><run cmd=\"x\">", "never closed"),
        (
            "<workflow><agent use=\"a\" prompt-file=\"p.md\">inline</agent></workflow>",
            "not both",
        ),
    ];
    for (text, expected) in cases {
        let err = parse(text).unwrap_err();
        assert!(err.contains(expected), "missing '{expected}' in:\n{err}");
    }
}

#[test]
fn loads_prompt_files_through_the_reader() {
    let reader = |path: &str| -> Result<String, String> {
        (path == "prompts/scan.md")
            .then(|| "Scan {{profile}}\n".to_string())
            .ok_or_else(|| "missing".to_string())
    };
    let wf = parse_workflow(
        "w",
        &wrap(r#"<agent use="scout" prompt-file="prompts/scan.md"/>"#),
        &reader,
    )
    .unwrap();
    let NodeKind::Agent(agent) = &wf.body[0].kind else {
        panic!("expected agent");
    };
    assert_eq!(agent.prompt, "Scan {{profile}}");
    let missing = wrap(r#"<agent use="scout" prompt-file="prompts/nope.md"/>"#);
    assert!(
        parse_workflow("w", &missing, &reader)
            .unwrap_err()
            .contains("cannot read prompt-file")
    );
}

#[test]
fn splits_commands_on_whitespace_with_quotes() {
    let cases: [(&str, &[&str]); 4] = [
        ("make all", &["make", "all"]),
        ("git commit -m 'a b' \"c d\"", &["git", "commit", "-m", "a b", "c d"]),
        ("echo ''", &["echo", ""]),
        ("  x  ", &["x"]),
    ];
    for (cmd, expected) in cases {
        assert_eq!(split_command(cmd).unwrap(), expected, "{cmd}");
    }
    assert!(split_command("   ").unwrap_err().contains("empty"));
}

#[test]
fn choose_and_call_parse_cases_and_inputs() {
    let text = wrap(
        r#"<run id="kind" cmd="echo bug"/>
<choose id="route" on="{{kind.output}}">
  <case value="bug"><call workflow="fix" profile="{{profile}}"/></case>
  <otherwise><run cmd="echo other"/></otherwise>
</choose>"#,
    );
    let wf = parse(&text).unwrap();
    let NodeKind::Choose(choose) = &wf.body[1].kind else {
        panic!("expected choose");
    };
    assert_eq!(choose.cases[0].0, "bug");
    let NodeKind::Call(call) = &choose.cases[0].1[0].kind else {
        panic!("expected call");
    };
    assert_eq!(call.workflow, "fix");
    assert_eq!(call.with["profile"], "{{profile}}");
    assert!(choose.otherwise.is_some());
}

#[test]
fn agent_use_and_model_may_be_templates_but_literal_models_are_checked() {
    let wf = parse(&wrap(
        r#"<each id="h" items="a b" as="cat"><agent use="hunter-{{cat}}" model="{{profile}}">Go.</agent></each>"#,
    ))
    .unwrap();
    let NodeKind::Each(each) = &wf.body[0].kind else {
        panic!("expected each");
    };
    let NodeKind::Agent(agent) = &each.body[0].kind else {
        panic!("expected agent");
    };
    assert_eq!(
        (agent.agent.as_str(), agent.model.as_deref()),
        ("hunter-{{cat}}", Some("{{profile}}"))
    );
    let err = parse(&wrap(r#"<agent use="x" model="huge">Go.</agent>"#)).unwrap_err();
    assert!(err.contains("model must be one of"), "{err}");
    let err = parse(&wrap(r#"<agent use="x-{{nope}}">Go.</agent>"#)).unwrap_err();
    assert!(err.contains("unknown reference '{{nope}}'"), "{err}");
}

#[test]
fn write_steps_need_a_path_and_value_and_check_references() {
    let wf = parse(&wrap(
        r#"<write id="saved" path=".rho/{{profile}}.md" value="{{profile}}"/>"#,
    ))
    .unwrap();
    let NodeKind::Write(write) = &wf.body[0].kind else {
        panic!("expected write");
    };
    assert_eq!(
        (write.path.as_str(), write.content.as_str()),
        (".rho/{{profile}}.md", "{{profile}}")
    );
    let err = parse(&wrap(r#"<write path=".rho/a"/>"#)).unwrap_err();
    assert!(err.contains("missing required attribute 'value'"), "{err}");
    let err = parse(&wrap(r#"<write path=".rho/a" value="{{ghost}}"/>"#)).unwrap_err();
    assert!(err.contains("unknown reference '{{ghost}}'"), "{err}");
}

#[test]
fn filters_are_validated_at_load_time() {
    let ok = wrap(r#"<output name="n" value="{{profile | count}}"/><run cmd="echo"/>"#);
    assert!(parse(&ok).is_ok());
    let bad = wrap(r#"<output name="n" value="{{profile | shout}}"/><run cmd="echo"/>"#);
    assert!(parse(&bad).unwrap_err().contains("unknown filter 'shout'"));
    let ghost = wrap(r#"<output name="n" value="{{profile | default:ghost}}"/><run cmd="echo"/>"#);
    assert!(parse(&ghost).unwrap_err().contains("unknown reference '{{ghost}}'"));
}

#[test]
fn run_is_a_reserved_name_bound_by_the_engine() {
    let ok = wrap(r#"<run cmd="echo {{run.id}}"/>"#);
    assert!(parse(&ok).is_ok());
    let input = "<workflow><input name=\"run\"/><run cmd=\"echo\"/></workflow>";
    assert!(parse(input).unwrap_err().contains("invalid input name 'run'"));
    let id = wrap(r#"<run id="run" cmd="echo"/>"#);
    assert!(parse(&id).unwrap_err().contains("already an input name"));
}

#[test]
fn resume_must_be_a_positive_integer_and_defaults_to_none() {
    let wf = parse(&wrap(
        r#"<agent use="a">Go.</agent><agent use="b" resume="2">Go.</agent>"#,
    ))
    .unwrap();
    let resumes: Vec<usize> = wf
        .body
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Agent(a) => Some(a.resume),
            _ => None,
        })
        .collect();
    assert_eq!(resumes, [0, 2]);
    let err = parse(&wrap(r#"<agent use="a" resume="0">Go.</agent>"#)).unwrap_err();
    assert!(err.contains("'resume' must be a positive integer"), "{err}");
}
