use super::filter::filter_lines;
use crate::tools::types::ToolResult;
use rho_harness_core::args::ScriptArgs;
use rho_harness_core::error::AppError;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ToolRunner: Send + Sync {
    fn run<'a>(&'a self, args: serde_json::Value) -> BoxFuture<'a, Result<String, String>>;
}

impl<F, Fut> ToolRunner for F
where
    F: Send + Sync + Fn(serde_json::Value) -> Fut,
    Fut: Send + Future<Output = Result<String, String>> + 'static,
{
    fn run<'a>(&'a self, args: serde_json::Value) -> BoxFuture<'a, Result<String, String>> {
        Box::pin((self)(args))
    }
}

#[derive(Clone, Default)]
pub struct ScriptDispatcher {
    runners: BTreeMap<String, Arc<dyn ToolRunner>>,
}

impl ScriptDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, runner: Arc<dyn ToolRunner>) {
        self.runners.insert(name.into(), runner);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ToolRunner>> {
        self.runners.get(name)
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.runners.contains_key(name)
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.runners.keys().cloned().collect()
    }
}

pub async fn execute_script(
    dispatcher: &ScriptDispatcher,
    args: ScriptArgs,
    max_output_bytes: usize,
) -> Result<ToolResult, AppError> {
    if args.steps.is_empty() {
        return Ok(ToolResult::error("No steps provided in script"));
    }

    let mut step_outputs = Vec::new();
    let mut script_failed = false;

    for (i, step) in args.steps.into_iter().enumerate() {
        let step_num = i + 1;
        if step.tool == "script" {
            step_outputs.push(format!(
                "[Step {step_num}: script Failed]\nNested script calls are not allowed"
            ));
            script_failed = true;
            break;
        }

        let Some(runner) = dispatcher.get(&step.tool) else {
            step_outputs.push(format!(
                "[Step {step_num}: {} Failed]\nUnknown tool: {}",
                step.tool, step.tool
            ));
            script_failed = true;
            break;
        };

        match runner.run(step.args).await {
            Ok(raw) => {
                let pipe_repr = format_pipe_header(&step.tool, step.filter.as_deref(), step.context);
                if let Some(ref pattern) = step.filter {
                    let context_lines = step.context.unwrap_or(2);
                    match filter_lines(&raw, pattern, context_lines) {
                        Ok(filtered) => {
                            step_outputs.push(format!("[Step {step_num}: {pipe_repr}]\n{}", filtered.text));
                        }
                        Err(err) => {
                            step_outputs.push(format!("[Step {step_num}: {pipe_repr} Filter Error]\n{err}"));
                            script_failed = true;
                            break;
                        }
                    }
                } else {
                    step_outputs.push(format!("[Step {step_num}: {pipe_repr}]\n{raw}"));
                }
            }
            Err(err) => {
                let pipe_repr = format_pipe_header(&step.tool, step.filter.as_deref(), step.context);
                step_outputs.push(format!("[Step {step_num}: {pipe_repr} Failed]\n{err}"));
                script_failed = true;
                break;
            }
        }
    }

    let full_content = step_outputs.join("\n\n");
    let bounded_content = if full_content.len() > max_output_bytes {
        let mut truncated = full_content[..max_output_bytes].to_string();
        truncated.push_str("\n\n[Script output truncated to maximum size limit]");
        truncated
    } else {
        full_content
    };

    if script_failed {
        Ok(ToolResult::error(bounded_content))
    } else {
        Ok(ToolResult::success(bounded_content))
    }
}

pub fn format_pipe_header(tool: &str, filter: Option<&str>, context: Option<usize>) -> String {
    let Some(pattern) = filter else {
        return tool.to_string();
    };
    let ctx = context.unwrap_or(2);
    if ctx > 0 {
        format!("{tool} | grep -C {ctx} {pattern:?}")
    } else {
        format!("{tool} | grep {pattern:?}")
    }
}
