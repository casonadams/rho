use super::command::CANCELLED;
use super::executor::{Executor, Frame, Scope};
use crate::provider::adapter::ModelCompletionRequest;
use crate::tools::subagent::RunnerSpec;
use crate::tools::subagent::runner::RUN_FOOTER_PREFIX;
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

fn strip_footer(text: &str) -> &str {
    text.rsplit_once(RUN_FOOTER_PREFIX).map_or(text, |(body, _)| body)
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
            })?
            .with_monitor_identity(&frame.path, &monitor_label);
        let model = runner.model_label();
        self.log.event(
            "agent_started",
            json!({ "step": position, "agent": agent.agent, "model": model }),
        );
        let result = runner.run(&build_prompt(agent, scope), None).await;
        if self.cancelled() {
            return Err(CANCELLED.to_string());
        }
        if result.is_error {
            return Err(result.content);
        }
        let text = strip_footer(&result.content).trim();
        let value = match agent.output {
            OutputFormat::Text => Value::String(text.to_string()),
            OutputFormat::Json => self.parse_json(text).await?,
        };
        let detail = format!("{model} · {}", summarize(&value));
        Ok((value, detail))
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

    #[test]
    fn strips_the_subagent_run_footer() {
        let text = format!("answer{RUN_FOOTER_PREFIX}2 turns, 1 tool calls, 10 tokens]");
        assert_eq!(strip_footer(&text), "answer");
        assert_eq!(strip_footer("plain"), "plain");
    }
}
