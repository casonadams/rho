use super::command::CANCELLED;
use super::executor::{Executor, Frame, Scope};
use crate::provider::adapter::ModelCompletionRequest;
use crate::tools::subagent::RunnerSpec;
use crate::tools::subagent::conversation::{ConversationSettings, Exchange};
use crate::tools::subagent::handoff::settle;
use crate::tools::subagent::runner::{Round, RoundEnd, RoundInput, SubagentRunner};
use crate::worktree::{Worktree, WorktreeLease};
use rho_harness_core::workflow::template::value_to_text;
use rho_harness_core::workflow::{AgentNode, OutputFormat};
use serde_json::{Value, json};

const JSON_INSTRUCTION: &str = "Respond with exactly one JSON object in a ```json fenced block.";
const REPAIR_PREAMBLE: &str = "Convert the user's text into exactly one JSON object. Reply with only the JSON object.";

fn context_text(value: &Value) -> String {
    match value {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_string) => items
            .iter()
            .enumerate()
            .map(|(i, item)| format!("### Item {}\n{}", i + 1, value_to_text(item)))
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => value_to_text(other),
    }
}

pub(crate) fn build_prompt(agent: &AgentNode, scope: &Scope) -> String {
    let mut prompt = scope.render(&agent.prompt).trim().to_string();
    for id in &agent.context {
        prompt.push_str(&format!("\n\n## Context: {id}\n{}", context_text(&scope.get(id))));
    }
    if agent.output == OutputFormat::Json {
        prompt.push_str(&format!("\n\n{JSON_INSTRUCTION}"));
    }
    prompt
}

pub(crate) fn extract_json(text: &str) -> Option<Value> {
    let fenced = text.split("```json").nth(1).and_then(|rest| rest.split("```").next());
    let bare = text.find('{').zip(text.rfind('}')).and_then(|(s, e)| text.get(s..=e));
    [fenced, bare]
        .into_iter()
        .flatten()
        .filter_map(|candidate| serde_json::from_str::<Value>(candidate.trim()).ok())
        .find(Value::is_object)
}

/// One workflow agent's round: its saved task, and the worktree (with its committed head) holding its
/// changes if it made any.
struct AgentRun {
    round: Round,
    task_id: Option<String>,
    worktree: Option<(Worktree, String)>,
}

/// Why an agent step produced no value: the round did not end with a final answer.
fn unfinished_step(run: &AgentRun) -> String {
    let mut message = match &run.round.end {
        RoundEnd::Stopped { reason, summary } => format!("agent {reason} before finishing.\n\nWork so far:\n{summary}"),
        _ => match run.round.body() {
            Ok(text) | Err(text) => text,
        },
    };
    if let Some(id) = &run.task_id {
        message.push_str(&format!(
            "\n\n(Agent task `{id}` was saved; continue it with the subagent tool.)"
        ));
    }
    if let Some((worktree, _)) = &run.worktree {
        message.push_str(&format!(
            "\n(Its unmerged changes are in worktree `{}`; review, merge, or discard them with the worktree tool.)",
            worktree.id
        ));
    }
    message
}

fn summarize(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .iter()
            .take(4)
            .map(|(k, v)| match v {
                Value::Array(items) => format!("{k}: {}", items.len()),
                Value::Object(_) => format!("{k}: {{…}}"),
                other => format!("{k}: {}", value_to_text(other)),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Value::String(text) => format!("{} lines", text.lines().count()),
        other => value_to_text(other),
    }
}

impl Executor<'_> {
    pub(crate) async fn run_agent(
        &self,
        frame: &Frame<'_>,
        label: &str,
        agent: &AgentNode,
        scope: &Scope,
    ) -> Result<(Value, String), String> {
        let position = frame.child(label).path;
        let monitor_label = if label == agent.agent {
            label.to_string()
        } else {
            format!("{label} · {}", agent.agent)
        };
        let runner = self
            .subagents
            .runner_for(RunnerSpec {
                agent: Some(&agent.agent),
                local_agents: &frame.bundle.agents,
                turns: Some(agent.turns),
                model: agent.model,
                position: Some(position.clone()),
                task_name: None,
            })
            .await?
            .with_monitor_identity(&frame.path, &monitor_label);
        let model = runner.model_label();
        self.log.event(
            "agent_started",
            json!({ "step": position, "agent": agent.agent, "model": model }),
        );
        let settings = ConversationSettings {
            name: crate::tools::subagent::task_name(label),
            agent: Some(agent.agent.clone()),
            model: agent.model.map(|m| m.as_str().to_string()),
            agent_file: frame
                .bundle
                .agents
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(&agent.agent))
                .map(|a| a.location.clone()),
        };
        let started = std::time::Instant::now();
        let run = self
            .agent_round(&runner, &settings, &position, &build_prompt(agent, scope))
            .await?;
        self.log
            .agent_files(&position, &run.round, started..std::time::Instant::now());
        if self.cancelled() || run.round.end == RoundEnd::Cancelled {
            return Err(CANCELLED.to_string());
        }
        let RoundEnd::Answered(text) = &run.round.end else {
            return Err(unfinished_step(&run));
        };
        let value = match agent.output {
            OutputFormat::Text => Value::String(text.trim().to_string()),
            OutputFormat::Json => self.parse_json(text.trim()).await?,
        };
        if let Some((worktree, head)) = &run.worktree {
            self.land(&position, worktree, head).await?;
        }
        let problems = match run.round.problem_count() {
            0 => String::new(),
            1 => "1 failed tool call · ".to_string(),
            n => format!("{n} failed tool calls · "),
        };
        let detail = format!("{model} · {problems}{}", summarize(&value));
        Ok((value, detail))
    }

    /// Merges a finished agent's worktree into the workflow's working tree; overlapping edits from
    /// agents that ran at the same time stop the workflow with conflict markers left to resolve.
    async fn land(&self, position: &str, worktree: &Worktree, head: &str) -> Result<(), String> {
        let outcome = worktree.merge_head_into(head, self.base_dir).await?;
        let files: Vec<&str> = outcome.changed.iter().map(|c| c.path.as_str()).collect();
        self.log.event(
            "agent_merged",
            json!({ "step": position, "worktree": worktree.id, "files": files, "conflicts": outcome.conflicts }),
        );
        if outcome.conflicts.is_empty() {
            return Ok(());
        }
        Err(format!(
            "{position}'s changes conflict with changes another agent already made: conflict markers were left in {} in {}. Resolve them, then merge the workflow's worktree.",
            outcome.conflicts.join(", "),
            self.base_dir.display()
        ))
    }

    /// Runs the agent in its own worktree (when worktrees are available), saving its round to the parent
    /// session as an A2A task when there is one.
    async fn agent_round(
        &self,
        runner: &SubagentRunner,
        settings: &ConversationSettings,
        position: &str,
        prompt: &str,
    ) -> Result<AgentRun, String> {
        let label = format!("workflow {position}");
        let Some(session) = &self.ctx().session else {
            return self.unsaved_round(runner, &label, prompt).await;
        };
        let mut exchange = Exchange::start(settings);
        if let Some(store) = self.subagents.worktrees() {
            let owner = session.session_id.as_str();
            exchange = exchange
                .with_worktree(store, self.base_dir, &label, Some(owner))
                .await?;
        }
        let outcome = exchange.exchange(runner, session, prompt, None).await;
        let state = outcome.task.status.state;
        self.log.event(
            "agent_task",
            json!({ "step": position, "task": outcome.task.id, "state": state.label(), "saved": outcome.save_error.is_none() }),
        );
        match &outcome.save_error {
            Some(error) => self
                .log
                .warn(format!("agent task for {position} was not saved: {error}")),
            None => self.log.task(position, &outcome.task.id, state),
        }
        if let Some(error) = &outcome.worktree_error {
            return Err(format!("{position}'s worktree could not be committed: {error}"));
        }
        Ok(AgentRun {
            task_id: outcome.continuable_id().map(str::to_string),
            worktree: outcome.worktree.zip(outcome.record.map(|r| r.head)),
            round: outcome.round,
        })
    }

    async fn unsaved_round(&self, runner: &SubagentRunner, label: &str, prompt: &str) -> Result<AgentRun, String> {
        let lease = match self.subagents.worktrees() {
            Some(store) => store.create(self.base_dir, label, None).await?.map(WorktreeLease::new),
            None => None,
        };
        let runner = match &lease {
            Some(lease) => runner.clone().in_worktree(&lease.worktree().path),
            None => runner.clone(),
        };
        let round = runner.run_round(Vec::new(), RoundInput::Prompt(prompt)).await;
        let worktree = match lease {
            Some(lease) => {
                let record = settle(lease.worktree()).await?;
                let worktree = lease.release();
                (!record.removed).then_some((worktree, record.head))
            }
            None => None,
        };
        Ok(AgentRun {
            round,
            task_id: None,
            worktree,
        })
    }

    async fn parse_json(&self, text: &str) -> Result<Value, String> {
        if let Some(value) = extract_json(text) {
            return Ok(value);
        }
        self.repair_json(text)
            .await
            .ok_or_else(|| "the agent did not return a JSON object".to_string())
    }

    async fn repair_json(&self, text: &str) -> Option<Value> {
        let model = self.subagents.model()?;
        let request = ModelCompletionRequest {
            preamble: Some(REPAIR_PREAMBLE.to_string()),
            prompt: text.to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
            temperature: Some(0.0),
        };
        let response = model.complete(request).await.ok()?;
        extract_json(&response.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn agent(context: &[&str], output: OutputFormat) -> AgentNode {
        AgentNode {
            agent: "critic".into(),
            prompt: "Check {{profile}}".into(),
            model: None,
            turns: 10,
            output,
            context: context.iter().map(|c| (*c).to_string()).collect(),
        }
    }

    #[test]
    fn prompt_renders_and_appends_context_and_json_instruction() {
        let mut inputs = BTreeMap::new();
        inputs.insert("profile".to_string(), "security".to_string());
        let mut scope = Scope::new(&inputs);
        scope.set("scan", json!(["first report", "second report"]));
        let prompt = build_prompt(&agent(&["scan"], OutputFormat::Json), &scope);
        assert_eq!(
            prompt,
            format!(
                "Check security\n\n## Context: scan\n### Item 1\nfirst report\n\n### Item 2\nsecond report\n\n{JSON_INSTRUCTION}"
            )
        );
        assert_eq!(build_prompt(&agent(&[], OutputFormat::Text), &scope), "Check security");
    }

    #[test]
    fn extracts_fenced_or_bare_json_objects() {
        let cases = [
            (
                "Done.\n```json\n{\"fixed\": [\"a\"]}\n```\nbye",
                Some(json!({"fixed": ["a"]})),
            ),
            ("Result: {\"actionable\": []} end", Some(json!({"actionable": []}))),
            ("```json\n[1, 2]\n```", None),
            ("no json here", None),
        ];
        for (text, expected) in cases {
            assert_eq!(extract_json(text), expected, "{text}");
        }
    }
}
