use super::builtins::stash_changes;
use super::executor::{CANCELLED, Executor, Frame, StepOutcome, summarize};
use super::report::StepStatus;
use super::shell::run_shell;
use crate::provider::adapter::ModelCompletionRequest;
use crate::tools::subagent::RunnerSpec;
use crate::tools::subagent::runner::RUN_FOOTER_PREFIX;
use rho_harness_core::workflow::template::{lookup, render, value_to_text};
use rho_harness_core::workflow::{AgentStep, Check, OnFail, OutputFormat, Step};
use serde_json::{Value, json};
use std::time::Duration;

const JSON_INSTRUCTION: &str = "Respond with exactly one JSON object in a ```json fenced block.";
const REPAIR_PREAMBLE: &str = "Convert the user's text into exactly one JSON object that follows the expected shape. Reply with only the JSON object.";

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

pub(crate) fn build_prompt(step: &Step, template: &str, scope: &Value) -> Result<String, String> {
    let mut prompt = render(template, scope)?.trim().to_string();
    for id in &step.context {
        let value = lookup(scope, id).unwrap_or(&Value::Null);
        prompt.push_str(&format!("\n\n## Context: {id}\n{}", context_text(value)));
    }
    if let Some(expected) = &step.expected_output {
        prompt.push_str(&format!("\n\n## Expected output\n{expected}"));
    }
    if step.output == OutputFormat::Json {
        prompt.push_str(&format!("\n\n{JSON_INSTRUCTION}"));
    }
    Ok(prompt)
}

fn with_feedback(prompt: &str, feedback: Option<&str>, attempt: usize) -> String {
    match feedback {
        None => prompt.to_string(),
        Some(feedback) => format!(
            "{prompt}\n\n## Verification failed (attempt {attempt})\nThe working tree still contains the previous attempt's changes.\n{feedback}\nFix the cause of this failure and run the verification again before you finish."
        ),
    }
}

pub(crate) fn extract_json(text: &str) -> Option<Value> {
    let fenced = text.split("```json").nth(1).and_then(|rest| rest.split("```").next());
    let candidates = [
        fenced,
        text.find('{').zip(text.rfind('}')).and_then(|(s, e)| text.get(s..=e)),
    ];
    candidates
        .into_iter()
        .flatten()
        .filter_map(|candidate| serde_json::from_str::<Value>(candidate.trim()).ok())
        .find(Value::is_object)
}

fn strip_footer(text: &str) -> &str {
    text.rsplit_once(RUN_FOOTER_PREFIX).map_or(text, |(body, _)| body)
}

impl Executor<'_> {
    pub(crate) async fn run_agent_step(
        &self,
        frame: &Frame<'_>,
        step: &Step,
        agent: &AgentStep,
        scope: &Value,
        index: Option<usize>,
    ) -> Result<StepOutcome, String> {
        let prompt = build_prompt(step, &agent.prompt, scope)?;
        let label = match index {
            Some(i) => format!("{} #{i} · {}", step.id, agent.agent),
            None => format!("{} · {}", step.id, agent.agent),
        };
        let attempts = step.check.as_ref().map_or(1, |c| c.retries + 1);
        let mut feedback: Option<String> = None;
        for attempt in 1..=attempts {
            let task = with_feedback(&prompt, feedback.as_deref(), attempt);
            let text = self.invoke_agent(frame, agent, &task, &label).await?;
            let value = self.parse_output(step, &text).await?;
            let Some(check) = &step.check else {
                return Ok(StepOutcome::done(value.clone(), summarize(&value)));
            };
            match self.verify(step, check, scope).await? {
                None => {
                    return Ok(StepOutcome::done(
                        value.clone(),
                        format!("{}; verified", summarize(&value)),
                    ));
                }
                Some(failure) => {
                    self.log.event(
                        "check_failed",
                        json!({ "step": frame.path, "id": step.id, "attempt": attempt, "failure": failure }),
                    );
                    feedback = Some(failure);
                }
            }
        }
        self.check_exhausted(frame, step, feedback.unwrap_or_default()).await
    }

    async fn invoke_agent(
        &self,
        frame: &Frame<'_>,
        agent: &AgentStep,
        task: &str,
        label: &str,
    ) -> Result<String, String> {
        let runner = self
            .subagents
            .runner_for(RunnerSpec {
                agent: Some(&agent.agent),
                local_agents: &frame.bundle.agents,
                max_turns: agent.max_turns,
                model: agent.model.as_deref(),
            })?
            .with_monitor_identity(&frame.path, label);
        let result = runner.run(task, None).await;
        if self.cancelled() {
            return Err(CANCELLED.to_string());
        }
        if result.is_error {
            return Err(result.content);
        }
        Ok(strip_footer(&result.content).trim().to_string())
    }

    async fn verify(&self, step: &Step, check: &Check, scope: &Value) -> Result<Option<String>, String> {
        let command = render(&check.command, scope)?;
        self.ctx().progress.stream_chunk(&format!("verifying `{command}`\n"));
        let outcome = run_shell(
            self.ctx(),
            self.base_dir,
            &command,
            Duration::from_secs(step.timeout_sec),
        )
        .await?;
        Ok((!outcome.success()).then(|| outcome.failure_summary(&command)))
    }

    async fn check_exhausted(&self, frame: &Frame<'_>, step: &Step, failure: String) -> Result<StepOutcome, String> {
        let on_fail = step.check.as_ref().map_or(OnFail::Stop, |c| c.on_fail);
        let stashed = match on_fail {
            OnFail::Stop => return Err(format!("verification kept failing: {failure}")),
            OnFail::Continue => false,
            OnFail::Stash => {
                let message = format!("rho workflow: failed {} › {}", frame.path, step.id);
                stash_changes(self.ctx(), self.base_dir, &message).await?
            }
        };
        let detail = if stashed {
            "verification failed; changes stashed (see `git stash list`)"
        } else {
            "verification failed"
        };
        Ok(StepOutcome {
            value: json!({ "failed": true, "stashed": stashed, "error": failure }),
            status: StepStatus::Failed,
            detail: detail.to_string(),
        })
    }

    async fn parse_output(&self, step: &Step, text: &str) -> Result<Value, String> {
        if step.output == OutputFormat::Text {
            return Ok(Value::String(text.to_string()));
        }
        if let Some(value) = extract_json(text) {
            return Ok(value);
        }
        self.repair_json(step, text)
            .await
            .ok_or_else(|| format!("step '{}' did not return a JSON object", step.id))
    }

    async fn repair_json(&self, step: &Step, text: &str) -> Option<Value> {
        let model = self.subagents.model()?;
        let expected = step.expected_output.as_deref().unwrap_or("a JSON object");
        let request = ModelCompletionRequest {
            preamble: Some(REPAIR_PREAMBLE.to_string()),
            prompt: format!("Expected shape:\n{expected}\n\nText:\n{text}"),
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
    use rho_harness_core::workflow::parse_workflow;
    use rho_harness_core::workflow::{StepKind, Workflow};

    fn workflow(text: &str) -> Workflow {
        parse_workflow("t", text, &|_| Err("no files".into())).unwrap()
    }

    #[test]
    fn prompt_includes_context_sections_expected_output_and_json_instruction() {
        let wf = workflow(
            "[[step]]\nid = \"scan\"\nrun = \"x\"\n[[step]]\nid = \"v\"\nagent = \"critic\"\ncontext = [\"scan\"]\noutput = \"json\"\nexpected_output = \"{\\\"a\\\": []}\"\nprompt = \"Check {{round}}\"",
        );
        let step = &wf.steps[1];
        let StepKind::Agent(agent) = &step.kind else {
            panic!("expected agent");
        };
        let scope = json!({"round": 2, "scan": ["first report", "second report"]});
        let prompt = build_prompt(step, &agent.prompt, &scope).unwrap();
        assert_eq!(
            prompt,
            format!(
                "Check 2\n\n## Context: scan\n### Item 1\nfirst report\n\n### Item 2\nsecond report\n\n## Expected output\n{{\"a\": []}}\n\n{JSON_INSTRUCTION}"
            )
        );
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

    #[test]
    fn feedback_is_appended_only_after_a_failed_attempt() {
        assert_eq!(with_feedback("do it", None, 1), "do it");
        let retried = with_feedback("do it", Some("`make` exited with code 2."), 2);
        assert!(retried.starts_with("do it\n\n## Verification failed (attempt 2)"));
        assert!(retried.contains("`make` exited with code 2."));
    }

    #[test]
    fn strips_the_subagent_run_footer() {
        let text = format!("answer{RUN_FOOTER_PREFIX}2 turns, 1 tool calls, 10 tokens]");
        assert_eq!(strip_footer(&text), "answer");
        assert_eq!(strip_footer("plain"), "plain");
    }
}
