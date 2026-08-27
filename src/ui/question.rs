use async_trait::async_trait;

use crate::error::AppError;
use crate::tools::{InteractiveQuestionPort, QuestionPort, UserAnswer, UserQuestion};
use crate::ui::interactive::{InteractionOption, InteractionPrompt, InteractionResponse, InteractiveUi};

pub fn question_port(ui: Option<InteractiveUi>) -> QuestionPort {
    match ui {
        Some(ui) => QuestionPort::new(UiQuestionPort(ui)),
        None => QuestionPort::new(TerminalQuestionPort),
    }
}

struct UiQuestionPort(InteractiveUi);

#[async_trait]
impl InteractiveQuestionPort for UiQuestionPort {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError> {
        let response = self
            .0
            .request(InteractionPrompt {
                title: question.header.unwrap_or_else(|| "Question".to_string()),
                body: question.question,
                options: question
                    .options
                    .into_iter()
                    .map(|option| InteractionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
                initial_selection: 0,
                allow_custom: question.allow_custom,
            })
            .await
            .map_err(|_| AppError::Cancelled("Question cancelled because the interactive UI closed".to_string()))?;
        Ok(match response {
            InteractionResponse::Selected(index) => UserAnswer::Selected(index),
            InteractionResponse::Custom(answer) => UserAnswer::Custom(answer),
            InteractionResponse::Cancelled => UserAnswer::Cancelled,
        })
    }
}

struct TerminalQuestionPort;

#[async_trait]
impl InteractiveQuestionPort for TerminalQuestionPort {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError> {
        tokio::task::spawn_blocking(move || prompt_terminal(question))
            .await
            .map_err(|error| AppError::Other(error.into()))?
    }
}

fn prompt_terminal(question: UserQuestion) -> Result<UserAnswer, AppError> {
    if let Some(header) = question.header {
        println!("\n[{header}] {}\n", question.question);
    } else {
        println!("\n{}\n", question.question);
    }

    if question.options.is_empty() {
        return inquire::Text::new("Your answer:")
            .prompt()
            .map(UserAnswer::Custom)
            .map_err(|_| AppError::Cancelled("Question cancelled by user".to_string()));
    }

    for option in &question.options {
        if let Some(description) = &option.description {
            println!("{}: {description}", option.label);
        }
    }
    let mut labels = question
        .options
        .iter()
        .map(|option| option.label.clone())
        .collect::<Vec<_>>();
    if question.allow_custom {
        labels.push("Type a custom answer...".to_string());
    }
    let selected = inquire::Select::new("Select an option:", labels)
        .prompt()
        .map_err(|_| AppError::Cancelled("Question cancelled by user".to_string()))?;
    if question.allow_custom && selected == "Type a custom answer..." {
        return inquire::Text::new("Your answer:")
            .prompt()
            .map(UserAnswer::Custom)
            .map_err(|_| AppError::Cancelled("Question cancelled by user".to_string()));
    }
    question
        .options
        .iter()
        .position(|option| option.label == selected)
        .map(UserAnswer::Selected)
        .ok_or_else(|| AppError::Cancelled("Question returned an invalid selection".to_string()))
}

#[cfg(test)]
mod tests {
    use super::question_port;
    use crate::tools::{UserAnswer, UserQuestion, UserQuestionOption};
    use crate::ui::interactive::{InteractionResponse, InteractiveUi, UiEvent};

    #[tokio::test]
    async fn ui_port_transports_options_descriptions_and_custom_answers() {
        let (ui, mut events) = InteractiveUi::channel();
        let port = question_port(Some(ui));
        let request = tokio::spawn(async move {
            port.ask(UserQuestion {
                question: "Choose?".to_string(),
                header: Some("Choice".to_string()),
                options: vec![UserQuestionOption {
                    label: "One".to_string(),
                    description: Some("First option".to_string()),
                }],
                allow_custom: true,
            })
            .await
        });

        let Some(UiEvent::Interaction { prompt, responder }) = events.recv().await else {
            panic!("expected question request");
        };
        assert_eq!(prompt.title, "Choice");
        assert_eq!(prompt.options[0].description.as_deref(), Some("First option"));
        assert!(prompt.allow_custom);
        responder
            .respond(InteractionResponse::Custom("custom".to_string()))
            .unwrap();
        assert_eq!(
            request.await.unwrap().unwrap(),
            UserAnswer::Custom("custom".to_string())
        );
    }

    #[tokio::test]
    async fn ui_port_preserves_cancellation() {
        let (ui, mut events) = InteractiveUi::channel();
        let port = question_port(Some(ui));
        let request = tokio::spawn(async move {
            port.ask(UserQuestion {
                question: "Continue?".to_string(),
                header: None,
                options: Vec::new(),
                allow_custom: true,
            })
            .await
        });
        let Some(UiEvent::Interaction { responder, .. }) = events.recv().await else {
            panic!("expected question request");
        };
        responder.respond(InteractionResponse::Cancelled).unwrap();
        assert_eq!(request.await.unwrap().unwrap(), UserAnswer::Cancelled);
    }
}
