use super::command::CANCELLED;
use super::executor::{Executor, Frame, Scope};
use super::resume::step_key;
use crate::engine::runner::turn::types::unless_cancelled;
use crate::provider::adapter::ModelCompletionRequest;
use crate::tools::subagent::conversation::{ConversationSettings, Exchange};
use crate::tools::subagent::handoff::{SettledRound, run_in_worktree};
use crate::tools::subagent::round::{Round, RoundEnd, RoundInput};
use crate::tools::subagent::runner::SubagentRunner;
use crate::tools::subagent::{RoundSlot, RunnerSpec};
use crate::worktree::{Worktree, WorktreeLease};
use rho_harness_core::session::SessionManager;
use rho_harness_core::workflow::template::value_to_text;
use rho_harness_core::workflow::{AgentNode, ModelRole, OutputFormat};
use serde_json::{Value, json};

const JSON_INSTRUCTION: &str = "Return exactly one JSON object: pass it as `data` to complete_task (a ```json fenced block in your final reply also works).";
const RESUME_PROMPT: &str = "You stopped before finishing. Do not start new work: call complete_task now with what you have found so far, putting any structured output in `data`.";
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

/// Where a stopped step's saved task and unmerged worktree can be found, if it has either.
fn handoff_note(run: &AgentRun) -> String {
    let mut note = String::new();
    if let Some(id) = &run.task_id {
        note.push_str(&format!(
            "\n\n(Agent task `{id}` was saved; continue it with the subagent tool.)"
        ));
    }
    if let Some((worktree, _)) = &run.worktree {
        note.push_str(&format!(
            "\n(Its unmerged changes are in worktree `{}`; review, merge, or discard them with the worktree tool.)",
            worktree.id
        ));
    }
    note
}

/// Why an agent step produced no value: the round did not end with a final answer.
fn unfinished_step(run: &AgentRun) -> String {
    let message = match &run.round.end {
        RoundEnd::Stopped { reason, summary } => format!("agent {reason} before finishing.\n\nWork so far:\n{summary}"),
        _ => match run.round.body() {
            Ok(text) | Err(text) => text,
        },
    };
    message + &handoff_note(run)
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
        let name = scope.render(&agent.agent);
        let name = name.trim();
        let prompt = build_prompt(agent, scope);
        let key = step_key(name, &prompt);
        if let Some((value, detail)) = self.replay.and_then(|replay| replay.get(&position, &key)) {
            self.log.event("agent_replayed", json!({ "step": position }));
            self.log.journal(&position, &key, value, detail);
            return Ok((value.clone(), format!("resumed · {detail}")));
        }
        let role = agent
            .model
            .as_deref()
            .map(|m| ModelRole::parse(scope.render(m).trim()))
            .transpose()?;
        let monitor_label = if label == name {
            label.to_string()
        } else {
            format!("{label} · {name}")
        };
        let runner = self
            .subagents
            .runner_for(RunnerSpec {
                agent: Some(name),
                local_agents: &frame.bundle.agents,
                turns: Some(agent.turns),
                model: role,
                position: Some(position.clone()),
                task_name: None,
                persona: agent.persona.as_deref(),
            })
            .await?
            .with_monitor_identity(&frame.path, &monitor_label);
        let model = runner.model_label();
        self.log.event(
            "agent_started",
            json!({ "step": position, "agent": name, "model": model }),
        );
        let settings = ConversationSettings {
            name: crate::tools::subagent::task_name(label),
            agent: Some(name.to_string()),
            model: role.map(|m| m.as_str().to_string()),
            agent_file: frame
                .bundle
                .agents
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(name))
                .map(|a| a.location.clone()),
            persona: agent.persona.clone(),
            turns: Some(agent.turns),
        };
        let started = std::time::Instant::now();
        let run = self.agent_round(&runner, &settings, &position, agent, &prompt).await?;
        self.log
            .agent_files(&position, &run.round, started..std::time::Instant::now());
        if self.cancelled() || run.round.end == RoundEnd::Cancelled {
            let note = handoff_note(&run);
            if !note.is_empty() {
                self.log.warn(format!("{position} was cancelled.{note}"));
            }
            return Err(CANCELLED.to_string());
        }
        let RoundEnd::Answered(text) = &run.round.end else {
            return Err(unfinished_step(&run));
        };
        let value = match (agent.output, &run.round.data) {
            (OutputFormat::Json, Some(data)) => data.clone(),
            (OutputFormat::Text, _) => Value::String(text.trim().to_string()),
            (OutputFormat::Json, None) => self
                .parse_json(&runner, text.trim())
                .await
                .map_err(|e| e + &handoff_note(&run))?,
        };
        let landed = match &run.worktree {
            Some((worktree, head)) => self.land(&position, worktree, head).await?,
            None => false,
        };
        let problems = crate::tools::subagent::evidence::failed_calls_text(run.round.problem_count())
            .map(|text| format!("{text} · "))
            .unwrap_or_default();
        let detail = format!("{model} · {problems}{}", summarize(&value));
        if !landed {
            self.log.journal(&position, &key, &value, &detail);
        }
        Ok((value, detail))
    }

    /// Merges a finished agent's worktree into the workflow tree; overlapping concurrent edits stop the workflow
    /// with conflict markers to resolve. Returns whether the agent changed any files.
    async fn land(&self, position: &str, worktree: &Worktree, head: &str) -> Result<bool, String> {
        let outcome = worktree.merge_head_into(head, self.base_dir).await?;
        let files: Vec<&str> = outcome.changed.iter().map(|c| c.path.as_str()).collect();
        self.log.event(
            "agent_merged",
            json!({ "step": position, "worktree": worktree.id, "files": files, "conflicts": outcome.conflicts }),
        );
        if outcome.conflicts.is_empty() {
            return Ok(!outcome.changed.is_empty());
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
        agent: &AgentNode,
        prompt: &str,
    ) -> Result<AgentRun, String> {
        let _slot = self.round_slot().await?;
        let label = format!("workflow {position}");
        let Some(session) = &self.ctx().session else {
            return self.unsaved_round(runner, position, &label, prompt).await;
        };
        let mut outcome = self
            .attached(Exchange::start(settings), &label, session)
            .await?
            .exchange(runner, session, prompt, None)
            .await;
        for attempt in 1..=agent.resume {
            if !matches!(outcome.round.end, RoundEnd::Stopped { .. }) || self.cancelled() {
                break;
            }
            let Some(context) = session.subagent_context(&outcome.task.id).await else {
                break;
            };
            self.log.event(
                "agent_resumed",
                json!({ "step": position, "task": outcome.task.id, "attempt": attempt }),
            );
            let next = Exchange::resume(context, &outcome.task.id)?.with_settings(settings);
            outcome = self
                .attached(next, &label, session)
                .await?
                .exchange(runner, session, RESUME_PROMPT, None)
                .await;
        }
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
            self.log
                .warn(format!("{position}'s worktree could not be committed: {error}"));
        }
        Ok(AgentRun {
            task_id: outcome.continuable_id().map(str::to_string),
            worktree: outcome.worktree.zip(outcome.record.map(|r| r.head)),
            round: outcome.round,
        })
    }

    /// Gives `exchange` the agent's worktree when worktrees are available.
    async fn attached(&self, exchange: Exchange, label: &str, session: &SessionManager) -> Result<Exchange, String> {
        match self.subagents.worktrees() {
            Some(store) => {
                exchange
                    .with_worktree(store, self.base_dir, label, Some(session.session_id.as_str()))
                    .await
            }
            None => Ok(exchange),
        }
    }

    async fn unsaved_round(
        &self,
        runner: &SubagentRunner,
        position: &str,
        label: &str,
        prompt: &str,
    ) -> Result<AgentRun, String> {
        let lease = match self.subagents.worktrees() {
            Some(store) => store.create(self.base_dir, label, None).await?.map(WorktreeLease::new),
            None => None,
        };
        let SettledRound {
            round,
            record,
            worktree,
        } = run_in_worktree(runner, lease, Vec::new(), RoundInput::Prompt(prompt))
            .await
            .settle()
            .await;
        let worktree = match record {
            Some(Ok(record)) => worktree
                .filter(|_| !record.removed)
                .map(|worktree| (worktree, record.head)),
            Some(Err(error)) => {
                self.log
                    .warn(format!("{position}'s worktree could not be committed: {error}"));
                None
            }
            None => None,
        };
        Ok(AgentRun {
            round,
            task_id: None,
            worktree,
        })
    }

    /// Waits for a free subagent round slot, giving up if the workflow is cancelled meanwhile.
    async fn round_slot(&self) -> Result<RoundSlot, String> {
        unless_cancelled(self.ctx().cancellation.as_ref(), self.subagents.wait_for_round_slot())
            .await
            .ok_or_else(|| CANCELLED.to_string())
    }

    async fn parse_json(&self, runner: &SubagentRunner, text: &str) -> Result<Value, String> {
        if let Some(value) = extract_json(text) {
            return Ok(value);
        }
        self.repair_json(runner, text)
            .await
            .ok_or_else(|| "the agent did not return a JSON object".to_string())
    }

    /// Asks the step's own model to restate `text` as JSON; a cancelled workflow skips the repair.
    async fn repair_json(&self, runner: &SubagentRunner, text: &str) -> Option<Value> {
        let request = ModelCompletionRequest {
            preamble: Some(REPAIR_PREAMBLE.to_string()),
            prompt: text.to_string(),
            messages: Default::default(),
            tools: Vec::new(),
            max_tokens: None,
            temperature: Some(0.0),
            thinking_level: None,
            conversation_id: None,
        };
        let repair = runner.model().complete(request);
        let response = unless_cancelled(self.ctx().cancellation.as_ref(), repair).await?.ok()?;
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
            persona: None,
            turns: 10,
            resume: 0,
            output,
            context: context.iter().map(|c| (*c).to_string()).collect(),
        }
    }

    #[test]
    fn prompt_renders_and_appends_context_and_json_instruction() {
        let mut inputs = BTreeMap::new();
        inputs.insert("profile".to_string(), "security".to_string());
        let mut scope = Scope::new(&inputs, "test-run");
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
