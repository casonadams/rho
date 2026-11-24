use super::*;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::permission::PermissionHook;
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::permission::tests::mock::MockHookPresenter;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use rho_harness_core::agents::AgentMonitor;
use rho_harness_core::error::AppError;
use std::sync::{Arc, Mutex};

/// A completion delivered the way a streaming provider sends it.
fn as_stream(response: ModelCompletionResponse) -> crate::provider::adapter::ModelEventStream {
    let mut events = Vec::new();
    if !response.content.is_empty() {
        events.push(Ok(ModelStreamEvent::Text(response.content)));
    }
    events.extend(
        response
            .tool_calls
            .into_iter()
            .map(|call| Ok(ModelStreamEvent::ToolCall(call))),
    );
    Box::pin(futures::stream::iter(events))
}

#[derive(Debug, Default)]
struct ScriptedModel {
    validate_replies: Mutex<Vec<String>>,
    requests: Mutex<Vec<(String, String)>>,
}

impl ScriptedModel {
    fn with_validate(replies: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            validate_replies: Mutex::new(replies.iter().map(|r| (*r).to_string()).collect()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn prompts_starting_with(&self, prefix: &str) -> Vec<String> {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|(_, p)| p.starts_with(prefix))
            .map(|(_, p)| p.clone())
            .collect()
    }

    fn preamble_for(&self, prefix: &str) -> String {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .find(|(_, p)| p.starts_with(prefix))
            .map(|(pre, _)| pre.clone())
            .unwrap()
    }

    fn reply(&self, prompt: &str) -> String {
        if prompt.starts_with("SCAN") {
            return format!("report: {}", prompt.lines().next().unwrap_or_default());
        }
        if prompt.starts_with("FIX") || prompt.starts_with("HELLO") {
            return "done".to_string();
        }
        let mut replies = self.validate_replies.lock().unwrap();
        if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies[0].clone()
        }
    }
}

#[async_trait::async_trait]
impl ModelAdapter for ScriptedModel {
    fn model_name(&self) -> &str {
        "scripted"
    }

    fn provider_name(&self) -> &str {
        "test"
    }

    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let preamble = request.preamble.clone().unwrap_or_default();
        let prompt = request.asked();
        self.requests.lock().unwrap().push((preamble, prompt.clone()));
        if prompt.starts_with("STRUCT") {
            let finish = rho_harness_core::model::ToolFunction::new(
                "complete_task",
                serde_json::json!({ "result": "found it", "data": { "answer": 42 } }),
            );
            return Ok(ModelCompletionResponse {
                content: String::new(),
                tool_calls: vec![rho_harness_core::model::ToolCall::new("call-finish", finish)],
                usage: Default::default(),
                finish_reason: None,
            });
        }
        Ok(ModelCompletionResponse {
            content: self.reply(&prompt),
            tool_calls: Vec::new(),
            usage: Default::default(),
            finish_reason: None,
        })
    }

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>
    {
        Ok(as_stream(self.complete(request).await?))
    }
}

const LOOP: &str = r#"
<workflow name="loop">
  <input name="focus" default="bugs"/>
  <loop id="round" max="4" while="{{validate.actionable}}" stop-if-unchanged="{{validate.actionable}}">
    <each id="scan" items="a b" as="slice" parallel="2">
      <agent use="scout">SCAN {{slice}} for {{focus}}</agent>
    </each>
    <agent id="validate" use="critic" output="json" context="scan">VALIDATE round {{round.index}} after {{validate.history}}</agent>
    <loop id="attempt" max="3" until="{{verify.ok}}" when="{{validate.actionable}}">
      <agent id="implement" use="general">FIX {{validate.actionable}} last failure: [{{verify.output}}]</agent>
      <run id="verify" cmd="sh -c 'test -f fixed-{{round.index}} &amp;&amp; echo passed || { touch fixed-{{round.index}}; echo broken-build; exit 1; }'"/>
    </loop>
  </loop>
  <output name="remaining" value="{{validate.actionable}}"/>
  <output name="rounds" value="{{round.passes}}"/>
</workflow>
"#;

const FOUND: &str = "```json\n{\"actionable\": [{\"fingerprint\": \"src/a.rs:bug\"}]}\n```";
const CLEAN: &str = "```json\n{\"actionable\": []}\n```";

struct Fixture {
    dir: tempfile::TempDir,
    model: Arc<ScriptedModel>,
    monitor: AgentMonitor,
    session: Option<rho_harness_core::session::SessionManager>,
}

impl Fixture {
    fn new(validate_replies: &[&str], files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(".agents/workflows").join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Self {
            dir,
            model: ScriptedModel::with_validate(validate_replies),
            monitor: AgentMonitor::default(),
            session: None,
        }
    }

    fn runner_with(
        &self,
        subagents: SubagentTool,
        project: Option<Arc<crate::engine::context::ProjectContext>>,
    ) -> WorkflowRunner {
        WorkflowRunner::new(subagents, self.dir.path(), None).bind(&TurnToolContext {
            cancellation: Some(CancellationSignal::default()),
            monitor: Some(self.monitor.clone()),
            project,
            session: self.session.clone(),
            ..TurnToolContext::default()
        })
    }

    fn runner(&self) -> WorkflowRunner {
        self.runner_with(SubagentTool::new(Some(self.model.clone()), Vec::new()), None)
    }

    async fn run(&self, name: &str, inputs: &[(&str, &str)]) -> WorkflowRun {
        let inputs = inputs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        self.runner().run(name, inputs).await
    }
}

const TEMPLATED: &str = r#"
<workflow name="templated">
  <each id="who" items="scout critic" as="a">
    <agent use="{{a}}">HELLO {{a}}</agent>
  </each>
  <write id="saved" path=".rho/out/report.md" value="{{who | count}} agents, {{who | compact | count}} answered"/>
  <output name="saved" value="{{saved}}"/>
</workflow>
"#;

const ESCAPING_WRITE: &str = r#"
<workflow name="escaping">
  <write path="src/main.rs" value="x"/>
</workflow>
"#;

const EACH_RESULT: &str = r#"
<workflow name="yielding">
  <each id="picked" items="a b" as="x" result="{{note | default:mark}}">
    <run id="mark" cmd="echo mark-{{x}}"/>
    <choose on="{{x}}">
      <case value="a"><run id="note" cmd="echo note-{{x}}"/></case>
    </choose>
  </each>
  <output name="picked" value="{{picked}}"/>
  <output name="text" value="This sentence is deliberately long enough to be printed as a paragraph rather than inside the JSON block of outputs"/>
  <output name="count" value="{{picked | count}}"/>
</workflow>
"#;

#[tokio::test]
async fn each_result_sets_what_each_item_yields_and_text_outputs_print_as_paragraphs() {
    let fx = Fixture::new(&[], &[("yielding.xml", EACH_RESULT)]);
    let run = fx.run("yielding", &[]).await;

    assert!(run.ok, "{}", run.report);
    assert!(run.report.contains("\"output\": \"note-a\""), "{}", run.report);
    assert!(run.report.contains("\"output\": \"mark-b\""), "{}", run.report);
    assert!(run.report.contains("\"count\": 2"), "{}", run.report);
    assert!(run.report.contains("\ntext:\n"), "{}", run.report);
    assert!(!run.report.contains("\"text\""), "{}", run.report);
}

const RUN_ID_WRITE: &str = r#"
<workflow name="stamped">
  <write path=".rho/out/{{run.id}}.md" value="x"/>
</workflow>
"#;

#[tokio::test]
async fn run_id_names_files_and_matches_the_run_log() {
    let fx = Fixture::new(&[], &[("stamped.xml", RUN_ID_WRITE)]);
    let run = fx.run("stamped", &[]).await;

    assert!(run.ok, "{}", run.report);
    let written = std::fs::read_dir(fx.dir.path().join(".rho/out"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let name = written.file_name().into_string().unwrap();
    let id = name.strip_suffix(".md").unwrap();
    let logs: Vec<String> = std::fs::read_dir(fx.dir.path().join(".rho/workflows"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        logs.iter().any(|l| l == &format!("stamped-{id}.jsonl")),
        "{id} {logs:?}"
    );
}

#[tokio::test]
async fn templated_agents_and_write_steps_persist_output_in_the_project() {
    let fx = Fixture::new(&[], &[("templated.xml", TEMPLATED)]);
    let run = fx.run("templated", &[]).await;

    assert!(run.ok, "{}", run.report);
    assert_eq!(fx.model.prompts_starting_with("HELLO").len(), 2);
    assert!(fx.monitor.summaries().iter().any(|s| s.label == "agent · critic"));
    let saved = std::fs::read_to_string(fx.dir.path().join(".rho/out/report.md")).unwrap();
    assert_eq!(saved, "2 agents, 2 answered");
    assert!(run.report.contains(".rho/out/report.md"), "{}", run.report);
}

#[tokio::test]
async fn resume_replays_journaled_agent_results_without_calling_the_model() {
    let fx = Fixture::new(&[], &[("templated.xml", TEMPLATED)]);
    let first = fx.run("templated", &[]).await;
    assert!(first.ok, "{}", first.report);
    let asked = fx.model.prompts_starting_with("HELLO").len();
    assert_eq!(asked, 2);
    let id = std::fs::read_dir(fx.dir.path().join(".rho/workflows"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .find_map(|n| {
            n.strip_prefix("templated-")
                .and_then(|n| n.strip_suffix(".jsonl"))
                .filter(|id| !id.ends_with(".journal"))
                .map(str::to_string)
        })
        .expect("run log");

    let again = fx.runner().resume(&id).await;

    assert!(again.ok, "{}", again.report);
    assert_eq!(
        fx.model.prompts_starting_with("HELLO").len(),
        asked,
        "no agent ran again"
    );
    assert!(again.report.contains("resumed"), "{}", again.report);
    let missing = fx.runner().resume("nope").await;
    assert!(
        !missing.ok && missing.report.contains("no workflow run 'nope'"),
        "{}",
        missing.report
    );
}

#[tokio::test]
async fn write_steps_outside_the_output_root_stop_the_workflow() {
    let fx = Fixture::new(&[], &[("escaping.xml", ESCAPING_WRITE)]);
    let run = fx.run("escaping", &[]).await;

    assert!(!run.ok, "{}", run.report);
    assert!(run.report.contains("inside .rho/"), "{}", run.report);
    assert!(!fx.dir.path().join("src/main.rs").exists());
}

#[tokio::test]
async fn loops_until_clean_feeding_failures_and_history_into_later_passes() {
    let fx = Fixture::new(&[FOUND, CLEAN], &[("loop.xml", LOOP)]);
    let run = fx.run("loop", &[]).await;

    assert!(run.ok, "{}", run.report);
    assert!(run.report.contains("Workflow `loop` completed."), "{}", run.report);
    assert!(
        run.report.contains("✓ loop › round — 2 pass(es), stopped: while"),
        "{}",
        run.report
    );
    assert!(
        run.report
            .contains("✓ loop › round 1 › attempt — 2 pass(es), stopped: until"),
        "{}",
        run.report
    );
    assert!(
        run.report.contains("· loop › round 2 › attempt — condition not met"),
        "{}",
        run.report
    );
    assert!(run.report.contains("\"rounds\": 2"), "{}", run.report);
    assert_eq!(fx.model.prompts_starting_with("SCAN").len(), 4);

    let fixes = fx.model.prompts_starting_with("FIX");
    assert_eq!(fixes.len(), 2);
    assert!(fixes[0].ends_with("last failure: []"), "{}", fixes[0]);
    assert!(fixes[1].contains("broken-build"), "{}", fixes[1]);

    let validates = fx.model.prompts_starting_with("VALIDATE");
    assert!(
        validates[0].starts_with("VALIDATE round 1 after []"),
        "{}",
        validates[0]
    );
    assert!(
        validates[1].starts_with("VALIDATE round 2 after [\n  {\n    \"actionable\""),
        "{}",
        validates[1]
    );
    assert!(
        validates[0].contains("## Context: scan\n### Item 1\nreport: SCAN a for bugs"),
        "{}",
        validates[0]
    );
    assert!(
        validates[1].contains("src/a.rs:bug"),
        "round 2 sees round 1's value in history"
    );
    assert!(fixes[0].contains("src/a.rs:bug"));

    let summaries = fx.monitor.summaries();
    assert!(
        summaries
            .iter()
            .any(|s| s.group == "loop › round 1 › scan #2" && s.label == "scout")
    );
    assert!(
        summaries
            .iter()
            .any(|s| s.label == "implement · general" && s.model == "test/scripted")
    );

    let log_dir = fx.dir.path().join(".rho/workflows");
    let log = std::fs::read_dir(log_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension().is_some_and(|ext| ext == "jsonl") && !path.to_string_lossy().ends_with(".journal.jsonl")
        })
        .unwrap();
    let text = std::fs::read_to_string(log).unwrap();
    for event in ["run_started", "agent_started", "command", "run_finished"] {
        assert!(text.contains(&format!("\"event\":\"{event}\"")), "missing {event}");
    }
}

#[tokio::test]
async fn stops_a_loop_when_its_value_stops_changing() {
    let fx = Fixture::new(&[FOUND], &[("loop.xml", LOOP)]);
    let run = fx.run("loop", &[]).await;
    assert!(run.ok, "{}", run.report);
    assert!(
        run.report.contains("✓ loop › round — 2 pass(es), stopped: unchanged"),
        "{}",
        run.report
    );
}

#[tokio::test]
async fn workflows_ignore_permission_rules_for_steps_and_agents() {
    let wf =
        r#"<workflow><run id="hi" cmd="printf hello"/><agent use="general">HELLO {{hi.output}}</agent></workflow>"#;
    let fx = Fixture::new(&[CLEAN], &[("trusted.xml", wf)]);
    let scope = parse_scope_from_str("[permission.bash]\n\"*\" = \"deny\"\n").unwrap();
    let deny_all = PermissionHook::with_policy(
        Some(fx.dir.path().to_path_buf()),
        Arc::new(MockHookPresenter::new(false, None)),
        build_policy(None, Some(scope)),
    );
    let gated = TurnToolContext {
        permission: Some(Arc::new(deny_all)),
        ..Default::default()
    };
    let subagents = SubagentTool::new(Some(fx.model.clone()), Vec::new()).with_turn_context(&gated);
    let run = fx.runner_with(subagents, None).run("trusted", BTreeMap::new()).await;
    assert!(run.ok, "{}", run.report);
    assert_eq!(fx.model.prompts_starting_with("HELLO"), ["HELLO hello"]);
}

fn project(dir: &std::path::Path) -> Arc<crate::engine::context::ProjectContext> {
    Arc::new(crate::engine::context::ProjectContext {
        current_dir: dir.to_path_buf(),
        base_system_prompt: String::new(),
        instruction_files: vec![(
            "AGENTS.md".to_string(),
            "Never add comments that restate code.".to_string(),
        )],
        memory: String::new(),
        skills: Vec::new(),
        agents: Vec::new(),
        active_tools: Vec::new(),
        git_status: Some("## main".to_string()),
        os_info: "testos".to_string(),
        date_str: "2026-11-17".to_string(),
        seen_instruction_files: Default::default(),
        no_context_files: false,
        dynamic_instructions_count: 0,
        dynamic_instructions_bytes: 0,
        last_git_check: None,
    })
}

#[tokio::test]
async fn agents_are_grounded_with_project_instructions_environment_and_position() {
    let wf = r#"<workflow name="greet"><agent id="hello" use="helper">HELLO</agent></workflow>"#;
    let agent = "---\nname: helper\ntools: read\n---\nYou greet people.";
    let fx = Fixture::new(
        &[CLEAN],
        &[("greet/workflow.xml", wf), ("greet/agents/helper.md", agent)],
    );
    let subagents = SubagentTool::new(Some(fx.model.clone()), Vec::new());
    let run = fx
        .runner_with(subagents, Some(project(fx.dir.path())))
        .run("greet", BTreeMap::new())
        .await;
    assert!(run.ok, "{}", run.report);
    let preamble = fx.model.preamble_for("HELLO");
    for expected in [
        "You greet people.",
        "Never add comments that restate code.",
        "Platform: testos",
        "Today's date is 2026-11-17",
        "Git repository status: ## main",
        "You are running as step `greet › hello` of a workflow.",
    ] {
        assert!(preamble.contains(expected), "missing {expected:?} in:\n{preamble}");
    }
}

#[tokio::test]
async fn an_agent_step_persona_adds_its_instructions_and_unknown_personas_stop_before_running() {
    let wf = r#"<workflow name="greet"><agent id="hello" use="critic" persona="skeptic">HELLO</agent></workflow>"#;
    let fx = Fixture::new(&[CLEAN], &[("greet/workflow.xml", wf)]);
    let skeptic = rho_harness_core::agents::PersonaMetadata {
        name: "skeptic".into(),
        description: String::new(),
        location: String::new(),
        instructions: "Demand evidence.".into(),
    };
    let without = SubagentTool::new(Some(fx.model.clone()), Vec::new());
    let refused = fx
        .runner_with(without.clone(), None)
        .run("greet", BTreeMap::new())
        .await;
    assert!(!refused.ok && !refused.started, "{}", refused.report);
    assert!(
        refused.report.contains("workflow 'greet': unknown persona 'skeptic'"),
        "{}",
        refused.report
    );
    assert!(fx.model.requests.lock().unwrap().is_empty());

    let run = fx
        .runner_with(without.with_personas(vec![skeptic]), None)
        .run("greet", BTreeMap::new())
        .await;
    assert!(run.ok, "{}", run.report);
    let preamble = fx.model.preamble_for("HELLO");
    assert!(preamble.contains("## Persona: skeptic\nDemand evidence."), "{preamble}");
}

#[tokio::test]
async fn blocks_and_calls_compose_without_any_agent() {
    let main = r#"
<workflow name="compose">
  <parallel id="both">
    <run id="a" cmd="printf bug"/>
    <run id="b" cmd="printf '{&quot;n&quot;: 2}'" output="json"/>
  </parallel>
  <choose id="route" on="{{a.output}}">
    <case value="feature"><run id="wrong" cmd="printf no"/></case>
    <case value="bug"><call id="fixed" workflow="fix" what="{{a.output}}"/></case>
  </choose>
  <each id="echoes" items="{{fixed.items}}" as="x"><run cmd="printf {{x}}"/></each>
  <run id="skipped" unless="{{both.b.json.n}}" cmd="printf never"/>
  <output name="route" value="{{route}}"/>
  <output name="n" value="{{b.json.n}}"/>
  <output name="echoes" value="{{echoes}}"/>
  <output name="skipped" value="{{skipped}}"/>
</workflow>"#;
    let fix = r#"<workflow name="fix"><input name="what"/><run id="r" cmd="printf '[&quot;fixed-{{what}}&quot;]'" output="json"/><output name="items" value="{{r.json}}"/></workflow>"#;
    let fx = Fixture::new(&[CLEAN], &[("compose/workflow.xml", main), ("compose/fix.xml", fix)]);
    let run = fx.run("compose", &[]).await;
    assert!(run.ok, "{}", run.report);
    assert!(run.report.contains("\"route\": \"bug\""), "{}", run.report);
    assert!(run.report.contains("\"n\": 2"), "{}", run.report);
    assert!(run.report.contains("\"output\": \"fixed-bug\""), "{}", run.report);
    assert!(run.report.contains("\"skipped\": null"), "{}", run.report);
    assert!(!run.report.contains("wrong"), "{}", run.report);
    assert!(fx.model.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_unknown_workflows_bad_inputs_and_unconfigured_models_before_running() {
    let slow = r#"<workflow name="slowpoke"><agent use="general" model="slow">HELLO</agent></workflow>"#;
    let fx = Fixture::new(&[CLEAN], &[("loop.xml", LOOP), ("slowpoke.xml", slow)]);
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case<'_>; 3] = [
        (
            "ghost",
            &[],
            "unknown workflow 'ghost'; available: audit, loop, slowpoke",
        ),
        ("loop", &[("nope", "1")], "no input named 'nope'"),
        ("slowpoke", &[], "model roles that are not configured: slow"),
    ];
    for (name, inputs, expected) in cases {
        let run = fx.run(name, inputs).await;
        assert!(!run.ok, "{name}");
        assert!(run.report.contains(expected), "{name}: {}", run.report);
    }
    assert!(fx.model.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failing_agent_stops_the_workflow_with_its_step_path() {
    let wf = r#"<workflow name="broken"><agent use="nobody">HELLO</agent><run cmd="printf after"/></workflow>"#;
    let fx = Fixture::new(&[CLEAN], &[("broken.xml", wf)]);
    let run = fx.run("broken", &[]).await;
    assert!(!run.ok);
    assert!(
        run.report
            .contains("Workflow `broken` stopped: broken › nobody: unknown subagent 'nobody'"),
        "{}",
        run.report
    );
    assert!(!run.report.contains("after"), "{}", run.report);
}

fn read_tool() -> Arc<dyn crate::tools::EngineTool> {
    Arc::new(crate::tools::GenericTool::new(
        "read",
        "reads",
        serde_json::json!({}),
        |_| async { Ok(crate::tools::ToolResult::success("fn diff() {}")) },
    ))
}

fn read_call() -> Vec<ModelStreamEvent> {
    vec![ModelStreamEvent::ToolCall(rho_harness_core::model::ToolCall::new(
        "call-read",
        rho_harness_core::model::ToolFunction::new("read", serde_json::json!({ "path": "a.rs" })),
    ))]
}

fn text_turn(text: &str) -> Vec<ModelStreamEvent> {
    vec![ModelStreamEvent::Text(text.to_string())]
}

const REVIEW: &str = r#"<workflow name="gate"><agent id="review" use="checker" turns="1" output="json">REVIEW the diff</agent><run cmd="printf after"/></workflow>"#;
const RESUMING: &str = r#"<workflow name="gate"><agent id="review" use="checker" turns="1" resume="1" output="json">REVIEW the diff</agent><output name="approved" value="{{review.approved}}"/></workflow>"#;
const STRUCTURED: &str = r#"<workflow name="structured"><agent id="s" use="scout" output="json">STRUCT go</agent><output name="answer" value="{{s.answer}}"/></workflow>"#;
const CHECKER: &str = "---\nname: checker\ntools: read\n---\nYou review diffs.";

#[tokio::test]
async fn structured_data_from_complete_task_is_the_json_value_without_parsing_text() {
    let fx = Fixture::new(&[CLEAN], &[("structured.xml", STRUCTURED)]);
    let run = fx.run("structured", &[]).await;

    assert!(run.ok, "{}", run.report);
    assert!(run.report.contains("\"answer\": 42"), "{}", run.report);
}

#[tokio::test]
async fn resume_continues_a_stopped_agent_in_the_same_task_and_takes_its_structured_result() {
    let mut fx = Fixture::new(
        &[CLEAN],
        &[("gate/workflow.xml", RESUMING), ("gate/agents/checker.md", CHECKER)],
    );
    let session = rho_harness_core::session::SessionManager::new(&fx.dir.path().join("sessions"), None).unwrap();
    fx.session = Some(session);
    let finish = vec![ModelStreamEvent::ToolCall(rho_harness_core::model::ToolCall::new(
        "call-finish",
        rho_harness_core::model::ToolFunction::new(
            "complete_task",
            serde_json::json!({ "result": "approved", "data": { "approved": true } }),
        ),
    ))];
    let adapter =
        crate::provider::adapter::MockAdapter::from_turns([read_call(), text_turn("So far: a.rs looks fine."), finish]);
    let subagents = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .with_foreground_budget(std::time::Duration::from_secs(60));

    let run = fx.runner_with(subagents, None).run("gate", BTreeMap::new()).await;

    assert!(run.ok, "{}", run.report);
    assert!(run.report.contains("\"approved\": true"), "{}", run.report);
    let sent = serde_json::to_string(&adapter.requests().last().unwrap().messages).unwrap();
    assert!(sent.contains("You stopped before finishing"), "{sent}");
    assert!(
        sent.contains("REVIEW the diff"),
        "the resumed round keeps the conversation: {sent}"
    );
}

#[tokio::test]
async fn an_agent_that_stops_early_fails_its_step_instead_of_producing_a_value() {
    let fx = Fixture::new(
        &[CLEAN],
        &[("gate/workflow.xml", REVIEW), ("gate/agents/checker.md", CHECKER)],
    );
    let adapter =
        crate::provider::adapter::MockAdapter::from_turns([read_call(), text_turn("So far: a.rs looks fine.")]);
    let subagents = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()]);

    let run = fx.runner_with(subagents, None).run("gate", BTreeMap::new()).await;

    assert!(!run.ok, "{}", run.report);
    assert!(
        run.report.contains(
            "Workflow `gate` stopped: gate › review: agent reached the turn limit of 1 before finishing.\n\nWork so far:\nSo far: a.rs looks fine."
        ),
        "{}",
        run.report
    );
    assert!(!run.report.contains("after"), "{}", run.report);
    assert_eq!(
        adapter.requests().len(),
        2,
        "no JSON repair call turns the summary into a value"
    );
    assert!(
        !run.report.contains("Agent tasks"),
        "nothing is saved without a session"
    );
}

#[tokio::test]
async fn workflow_agent_tasks_are_saved_and_can_be_continued_by_the_main_agent() {
    let mut fx = Fixture::new(
        &[CLEAN],
        &[("gate/workflow.xml", REVIEW), ("gate/agents/checker.md", CHECKER)],
    );
    let session = rho_harness_core::session::SessionManager::new(&fx.dir.path().join("sessions"), None).unwrap();
    fx.session = Some(session.clone());
    let adapter =
        crate::provider::adapter::MockAdapter::from_turns([read_call(), text_turn("So far: a.rs looks fine.")]);
    let subagents = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .with_foreground_budget(std::time::Duration::from_secs(60));

    let run = fx.runner_with(subagents, None).run("gate", BTreeMap::new()).await;

    let task_id = run
        .report
        .split("task `")
        .nth(1)
        .and_then(|rest| rest.split('`').next())
        .unwrap()
        .to_string();
    assert!(task_id.starts_with("review-"), "{}", run.report);
    assert!(run.report.contains("(Agent task `review-"), "{}", run.report);
    assert!(
        run.report
            .contains("Agent tasks (continue one with the subagent tool's `task_id`"),
        "{}",
        run.report
    );
    assert!(
        run.report
            .contains(&format!("gate › review — task `{task_id}` · input-required")),
        "{}",
        run.report
    );

    adapter.add_turn(vec![Ok(ModelStreamEvent::Text("{\"approved\": true}".into()))]);
    let main_agent_tool = SubagentTool::new(Some(Arc::new(adapter.clone())), vec![read_tool()])
        .with_foreground_budget(std::time::Duration::from_secs(60))
        .with_turn_context(&TurnToolContext {
            session: Some(session),
            ..Default::default()
        });
    let continued = crate::tools::EngineTool::execute(
        &main_agent_tool,
        serde_json::json!({ "task_id": task_id, "prompt": "continue" }),
    )
    .await
    .unwrap();

    assert!(!continued.is_error, "{}", continued.content);
    assert!(
        continued.content.starts_with("{\"approved\": true}"),
        "{}",
        continued.content
    );
    let request = adapter.requests().pop().unwrap();
    assert!(
        request
            .preamble
            .clone()
            .unwrap_or_default()
            .contains("You review diffs."),
        "the workflow-local agent is reloaded from its file"
    );
    assert_eq!(request.asked(), "continue");
    assert!(
        request.messages[0]
            == rho_harness_core::model::ChatMessage::user(
                "REVIEW the diff\n\nReturn exactly one JSON object: pass it as `data` to complete_task (a ```json fenced block in your final reply also works)."
            )
    );
}

/// Answers each agent by its own prompt, so parallel agents get their scripted turns in any order:
/// the first turn writes `a.rs` with the text after `WRITE `, later turns finish.
#[derive(Debug, Default)]
struct WritingModel;

#[async_trait::async_trait]
impl ModelAdapter for WritingModel {
    fn model_name(&self) -> &str {
        "writer"
    }

    fn provider_name(&self) -> &str {
        "test"
    }

    async fn complete(&self, request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let body = request
            .asked()
            .strip_prefix("WRITE ")
            .map(|rest| rest.lines().next().unwrap_or_default().to_string());
        let tool_calls: Vec<rho_harness_core::model::ToolCall> = body
            .iter()
            .map(|body| {
                rho_harness_core::model::ToolCall::new(
                    format!("call-{body}"),
                    rho_harness_core::model::ToolFunction::new(
                        "write",
                        serde_json::json!({ "path": "a.rs", "content": format!("{body}\n") }),
                    ),
                )
            })
            .collect();
        Ok(ModelCompletionResponse {
            content: if tool_calls.is_empty() {
                "done".into()
            } else {
                String::new()
            },
            tool_calls,
            usage: Default::default(),
            finish_reason: None,
        })
    }

    async fn stream(
        &self,
        request: ModelCompletionRequest,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>
    {
        Ok(as_stream(self.complete(request).await?))
    }
}

/// A repository holding `workflow`, and a workflow runner whose agents use `WritingModel` and real file tools.
fn worktree_runner(
    workflow: &str,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    crate::worktree::WorktreeStore,
    WorkflowRunner,
) {
    let (dir, root) =
        crate::worktree::git::test_repo::create(&[("a.rs", "base\n"), (".agents/workflows/w/workflow.xml", workflow)]);
    let store = crate::worktree::WorktreeStore::new(dir.path().join("store"));
    let placeholder: Arc<dyn crate::tools::EngineTool> = Arc::new(crate::tools::GenericTool::new(
        "write",
        "replaced by the real write tool in each worktree",
        serde_json::json!({}),
        |_| async { Ok(crate::tools::ToolResult::error("the placeholder must never run")) },
    ));
    let subagents = SubagentTool::new(Some(Arc::new(WritingModel)), vec![placeholder]).with_worktrees(store.clone());
    let runner = WorkflowRunner::new(subagents, &root, None).bind(&TurnToolContext {
        cancellation: Some(CancellationSignal::default()),
        ..TurnToolContext::default()
    });
    (dir, root, store, runner)
}

#[tokio::test]
async fn workflows_run_in_a_worktree_and_hand_their_changes_to_the_parent() {
    let workflow = r#"<workflow name="w"><agent id="fix" use="general">WRITE agent</agent><run id="check" cmd="cat a.rs"/><output name="seen" value="{{check.output}}"/></workflow>"#;
    let (_dir, root, store, runner) = worktree_runner(workflow);

    let run = runner.run("w", BTreeMap::new()).await;

    assert!(run.ok, "{}", run.report);
    assert!(
        run.report.contains("\"seen\": \"agent\""),
        "later steps see the agent's change: {}",
        run.report
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.rs")).unwrap(),
        "base\n",
        "the working tree is untouched"
    );
    let worktrees = store.list(&root).await;
    assert_eq!(
        worktrees.len(),
        1,
        "the agent's worktree was merged into the run's and removed"
    );
    let run_worktree = &worktrees[0];
    assert!(run_worktree.id.starts_with("workflow-w-"), "{}", run_worktree.id);
    assert!(
        run.report
            .contains(&format!("The workflow's changes are in worktree `{}`", run_worktree.id)),
        "{}",
        run.report
    );
    assert_eq!(
        std::fs::read_to_string(run_worktree.path.join("a.rs")).unwrap(),
        "agent\n"
    );
}

#[tokio::test]
async fn an_agent_whose_json_cannot_be_read_points_to_its_unmerged_worktree() {
    let workflow = r#"<workflow name="w"><agent id="fix" use="general" output="json">WRITE agent</agent></workflow>"#;
    let (_dir, root, store, runner) = worktree_runner(workflow);

    let run = runner.run("w", BTreeMap::new()).await;

    assert!(!run.ok, "{}", run.report);
    assert!(
        run.report.contains("the agent did not return a JSON object"),
        "{}",
        run.report
    );
    let listed = store.list(&root).await;
    let ids: Vec<&str> = listed.iter().map(|w| w.id.as_str()).collect();
    let agent_worktree = listed
        .iter()
        .find(|w| w.id.starts_with("workflow-w-fix-"))
        .unwrap_or_else(|| panic!("no agent worktree in {ids:?}; report: {}", run.report));
    assert!(
        run.report
            .contains(&format!("unmerged changes are in worktree `{}`", agent_worktree.id)),
        "{}",
        run.report
    );
}

#[tokio::test]
async fn parallel_agent_worktrees_with_overlapping_edits_stop_the_workflow() {
    let workflow = r#"<workflow name="w"><parallel><agent id="left" use="general">WRITE one</agent><agent id="right" use="general">WRITE two</agent></parallel><run cmd="printf after"/></workflow>"#;
    let (_dir, root, store, runner) = worktree_runner(workflow);

    let run = runner.run("w", BTreeMap::new()).await;

    assert!(!run.ok, "{}", run.report);
    assert!(
        run.report
            .contains("changes conflict with changes another agent already made: conflict markers were left in a.rs"),
        "{}",
        run.report
    );
    assert!(
        !run.report.contains("w › run"),
        "the later step never ran: {}",
        run.report
    );
    assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), "base\n");
    let run_worktree = store
        .list(&root)
        .await
        .into_iter()
        .find(|w| w.id.starts_with("workflow-w-"))
        .unwrap();
    let merged = std::fs::read_to_string(run_worktree.path.join("a.rs")).unwrap();
    assert!(
        merged.contains("<<<<<<<") && merged.contains("one") && merged.contains("two"),
        "{merged}"
    );
}
