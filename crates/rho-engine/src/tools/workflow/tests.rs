use super::*;
use crate::permission::PermissionHook;
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::permission::tests::mock::MockHookPresenter;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use rho_harness_core::agents::AgentMonitor;
use std::sync::Mutex;

const WORKFLOW: &str = r#"
description = "test loop"

[[input]]
name = "focus"
default = "bugs"

[[input]]
name = "check"
default = "true"

[[step]]
id = "rounds"
workflow = "round"
with = { focus = "{{inputs.focus}}", check = "{{inputs.check}}" }
repeat = { until_empty = "actionable", max = 4, stop_on_no_progress = true }
carry = ["excluded"]

[outputs]
remaining = "rounds.actionable"
excluded = "rounds.excluded"
"#;

const ROUND: &str = r#"
[[input]]
name = "focus"

[[input]]
name = "check"

[[input]]
name = "excluded"
default = []

[[step]]
id = "slices"
run = "printf '[\"a\", \"b\"]'"
output = "json"

[[step]]
id = "scan"
agent = "scout"
over = "slices"
prompt = "SCAN {{item}} for {{inputs.focus}}"

[[step]]
id = "validate"
agent = "critic"
context = ["scan"]
output = "json"
prompt = "VALIDATE excluded={{inputs.excluded}}"

[[step]]
id = "implement"
agent = "general"
when = "validate.actionable"
output = "json"
check = "{{inputs.check}}"
retries = 1
on_fail = "continue"
prompt = "FIX {{validate.actionable}}"

[outputs]
actionable = "validate.actionable"
excluded = ["inputs.excluded", "implement.wont_fix"]
"#;

const FIX_REPLY: &str = "Done.\n```json\n{\"fixed\": [\"src/a.rs:bug\"], \"wont_fix\": [{\"fingerprint\": \"src/b.rs:fp\", \"reason\": \"false positive\"}], \"files_changed\": [\"src/a.rs\"]}\n```";

#[derive(Debug, Default)]
struct ScriptedModel {
    validate_replies: Mutex<Vec<String>>,
    prompts: Mutex<Vec<String>>,
}

impl ScriptedModel {
    fn with_validate(replies: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            validate_replies: Mutex::new(replies.iter().map(|r| (*r).to_string()).collect()),
            prompts: Mutex::new(Vec::new()),
        })
    }

    fn prompts_starting_with(&self, prefix: &str) -> Vec<String> {
        let prompts = self.prompts.lock().unwrap();
        prompts.iter().filter(|p| p.starts_with(prefix)).cloned().collect()
    }

    fn reply(&self, prompt: &str) -> String {
        if prompt.starts_with("SCAN") {
            return format!("report: {}", prompt.lines().next().unwrap_or_default());
        }
        if prompt.starts_with("FIX") {
            return FIX_REPLY.to_string();
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
        self.prompts.lock().unwrap().push(request.prompt.clone());
        Ok(ModelCompletionResponse {
            content: self.reply(&request.prompt),
            tool_calls: Vec::new(),
            usage: Default::default(),
            finish_reason: None,
        })
    }

    async fn stream(
        &self,
        _request: ModelCompletionRequest,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>
    {
        Err(AppError::Other(anyhow::anyhow!("scripted model only completes")))
    }
}

const FOUND: &str = "```json\n{\"actionable\": [{\"fingerprint\": \"src/a.rs:bug\", \"severity\": \"high\"}]}\n```";
const CLEAN: &str = "```json\n{\"actionable\": []}\n```";

struct Fixture {
    dir: tempfile::TempDir,
    model: Arc<ScriptedModel>,
    monitor: AgentMonitor,
}

impl Fixture {
    fn new(validate_replies: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join(".agents/workflows/loop");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("workflow.toml"), WORKFLOW).unwrap();
        std::fs::write(bundle.join("round.toml"), ROUND).unwrap();
        Self {
            dir,
            model: ScriptedModel::with_validate(validate_replies),
            monitor: AgentMonitor::default(),
        }
    }

    fn tool(&self, ctx: TurnToolContext) -> Arc<dyn EngineTool> {
        let subagents = SubagentTool::new(Some(self.model.clone()), Vec::new());
        let ctx = TurnToolContext {
            monitor: Some(self.monitor.clone()),
            ..ctx
        };
        WorkflowTool::new(subagents, self.dir.path(), None)
            .bind_turn(&ctx)
            .unwrap()
    }

    async fn run(&self, inputs: Value) -> ToolResult {
        self.run_with(TurnToolContext::default(), inputs).await
    }

    async fn run_with(&self, ctx: TurnToolContext, inputs: Value) -> ToolResult {
        self.tool(ctx)
            .execute(json!({ "name": "loop", "inputs": inputs }))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn repeats_rounds_until_clean_and_carries_exclusions_forward() {
    let fx = Fixture::new(&[FOUND, CLEAN]);
    let result = fx.run(json!({})).await;

    assert!(!result.is_error, "{}", result.content);
    assert!(
        result.content.contains("Workflow `loop` completed."),
        "{}",
        result.content
    );
    assert!(
        result.content.contains("✓ loop › rounds — 2 round(s), stopped: clean"),
        "{}",
        result.content
    );
    assert!(
        result.content.contains("· loop › rounds › round 2 › implement"),
        "{}",
        result.content
    );
    assert_eq!(fx.model.prompts_starting_with("SCAN").len(), 4);
    let validates = fx.model.prompts_starting_with("VALIDATE");
    assert!(
        validates[0].contains("## Context: scan\n### Item 1\nreport: SCAN a for bugs"),
        "{}",
        validates[0]
    );
    assert!(!validates[0].contains("src/b.rs:fp"));
    assert!(
        validates[1].contains("src/b.rs:fp"),
        "round 2 must see the carried exclusion"
    );

    let summaries = fx.monitor.summaries();
    assert!(
        summaries
            .iter()
            .any(|s| s.group == "loop › rounds › round 1" && s.label == "scan #2 · scout")
    );
    assert!(summaries.iter().any(|s| s.label == "implement · general"));

    let log_dir = fx.dir.path().join(".rho/workflows");
    let log = std::fs::read_dir(log_dir).unwrap().next().unwrap().unwrap().path();
    let text = std::fs::read_to_string(log).unwrap();
    assert!(text.contains("\"event\":\"run_started\""));
    assert!(text.contains("\"event\":\"round_finished\""));
}

#[tokio::test]
async fn stops_when_a_round_makes_no_progress() {
    let fx = Fixture::new(&[FOUND]);
    let result = fx.run(json!({})).await;
    assert!(!result.is_error, "{}", result.content);
    assert!(
        result.content.contains("2 round(s), stopped: no progress"),
        "{}",
        result.content
    );
    assert!(result.content.contains("\"remaining\": ["), "{}", result.content);
}

#[tokio::test]
async fn failed_verification_retries_with_feedback_then_continues() {
    let fx = Fixture::new(&[FOUND, CLEAN]);
    let result = fx.run(json!({ "check": "echo broken-build; exit 1" })).await;

    assert!(!result.is_error, "{}", result.content);
    let fixes = fx.model.prompts_starting_with("FIX");
    assert_eq!(fixes.len(), 2);
    assert!(!fixes[0].contains("Verification failed"));
    assert!(fixes[1].contains("## Verification failed (attempt 2)"), "{}", fixes[1]);
    assert!(fixes[1].contains("broken-build"), "{}", fixes[1]);
    assert!(
        result
            .content
            .contains("✗ loop › rounds › round 1 › implement — verification failed"),
        "{}",
        result.content
    );
}

#[tokio::test]
async fn shell_steps_go_through_the_permission_gate() {
    let fx = Fixture::new(&[CLEAN]);
    let scope = parse_scope_from_str("[permission.bash]\n\"printf *\" = \"deny\"\n").unwrap();
    let hook = PermissionHook::with_policy(
        Some(fx.dir.path().to_path_buf()),
        Arc::new(MockHookPresenter::new(false, None)),
        build_policy(None, Some(scope)),
    );
    let ctx = TurnToolContext {
        permission: Some(Arc::new(hook)),
        ..Default::default()
    };
    let result = fx.run_with(ctx, json!({})).await;
    assert!(result.is_error);
    assert!(result.content.contains("Workflow `loop` stopped"), "{}", result.content);
    assert!(
        result.content.contains("✗ loop › rounds › round 1 › slices"),
        "{}",
        result.content
    );
    assert!(fx.model.prompts_starting_with("SCAN").is_empty());
}

#[tokio::test]
async fn rejects_unknown_workflows_and_bad_inputs_before_running() {
    let fx = Fixture::new(&[CLEAN]);
    let tool = fx.tool(TurnToolContext::default());
    let cases = [
        (json!({ "name": "ghost" }), "unknown workflow 'ghost'; available:"),
        (
            json!({ "name": "loop", "inputs": { "nope": 1 } }),
            "no input named 'nope'",
        ),
        (json!({ "name": "refine" }), "requires input 'profile'"),
        (json!({ "inputs": {} }), "failed to parse workflow arguments"),
    ];
    for (args, expected) in cases {
        let result = tool.execute(args.clone()).await.unwrap();
        assert!(result.is_error, "{args}");
        assert!(result.content.contains(expected), "{args}: {}", result.content);
    }
    assert!(fx.model.prompts.lock().unwrap().is_empty());
}

#[test]
fn description_lists_available_workflows_with_argument_hints() {
    let fx = Fixture::new(&[CLEAN]);
    let tool = WorkflowTool::new(SubagentTool::new(None, Vec::new()), fx.dir.path(), None);
    assert!(
        tool.description().contains("loop [focus] [check]"),
        "{}",
        tool.description()
    );
    assert!(
        tool.description().contains("refine <review|security"),
        "{}",
        tool.description()
    );
}
