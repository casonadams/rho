use crate::tools::ask_user::{InteractiveQuestionPort, UserAnswer, UserQuestion};
use async_trait::async_trait;
use rho_core::error::{AppError, Result};
use rho_sdk::contract::{InteractionOption, InteractionRequest, InteractionResponse, ToolHost};

pub(crate) struct HostQuestionPort<'a>(pub(crate) &'a dyn ToolHost);

#[async_trait]
impl InteractiveQuestionPort for HostQuestionPort<'_> {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer> {
        let response = self
            .0
            .interact(InteractionRequest {
                question: question.question,
                header: question.header,
                options: question
                    .options
                    .into_iter()
                    .map(|option| InteractionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
                allow_custom: question.allow_custom,
            })
            .await
            .map_err(|error| AppError::Tool(error.to_string()))?;
        Ok(match response {
            InteractionResponse::Selected(index) => UserAnswer::Selected(index),
            InteractionResponse::Custom(value) => UserAnswer::Custom(value),
            InteractionResponse::Cancelled => UserAnswer::Cancelled,
        })
    }
}
