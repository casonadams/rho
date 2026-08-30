//! Structured question ports shared by the ask-user tool capability and the
//! presentation layer.

use async_trait::async_trait;

use crate::error::Result;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestion {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<UserQuestionOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAnswer {
    Selected(usize),
    Custom(String),
    Cancelled,
}

#[async_trait]
pub trait InteractiveQuestionPort: Send + Sync {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer>;
}

#[derive(Clone)]
pub struct QuestionPort(Arc<dyn InteractiveQuestionPort>);

impl QuestionPort {
    pub fn new(port: impl InteractiveQuestionPort + 'static) -> Self {
        Self(Arc::new(port))
    }

    pub async fn ask(&self, question: UserQuestion) -> Result<UserAnswer> {
        self.0.ask(question).await
    }
}

#[async_trait]
impl InteractiveQuestionPort for QuestionPort {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer> {
        self.0.ask(question).await
    }
}
