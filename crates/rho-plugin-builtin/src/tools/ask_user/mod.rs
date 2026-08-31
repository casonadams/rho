pub mod spec;
#[cfg(test)]
mod tests;

pub use rho_core::args::AskUserArgs;
pub use rho_core::presentation::questions::{
    InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion, UserQuestionOption,
};

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rho_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use spec::{
    QuestionSpec, ask_question, extract_question_text, extract_str_from_map, extract_vec_from_map,
    prompt_question_value,
};

#[derive(Clone, Default)]
pub struct AskUserTool;

impl AskUserTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, port: &dyn InteractiveQuestionPort, args: AskUserArgs) -> Result<ToolResult, AppError> {
        if let Some(questions) = args.questions.as_deref().filter(|questions| !questions.is_empty()) {
            let mut results = Vec::with_capacity(questions.len());
            for (index, question) in questions.iter().enumerate() {
                results.push(prompt_question_value(port, question, index + 1).await?);
            }
            return Ok(ToolResult::success(results.join("\n")));
        }

        let question = extract_question_text(&args);
        let header = args.header.or_else(|| extract_str_from_map(&args.extra, "header"));
        let options = args
            .options
            .or_else(|| extract_vec_from_map(&args.extra, "options"))
            .or_else(|| extract_vec_from_map(&args.extra, "choices"));
        let answer = ask_question(
            port,
            QuestionSpec {
                question: &question,
                header,
                options: options.as_deref(),
            },
        )
        .await?;
        Ok(ToolResult::success(answer))
    }
}

impl Tool for AskUserTool {
    const NAME: &'static str = "ask_user";
    type Args = AskUserArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<AskUserArgs>()
    }

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let port: &QuestionPort = context
            .get()
            .ok_or_else(|| ToolExecutionError::other("Interactive question port is not configured"))?;
        into_rig_result(self.execute(port, args).await)
    }
}

#[derive(Clone, Default)]
pub struct AskUserQuestionTool(AskUserTool);

impl Tool for AskUserQuestionTool {
    const NAME: &'static str = "ask_user_question";
    type Args = AskUserArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<AskUserArgs>()
    }

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.0.call(context, args).await
    }
}
