use crate::auth::AuthStore;
use crate::config::Config;
use crate::error::{AppError, Result};
use crate::intent::store::{IntentHandle, IntentSummary, list_unfinished, workspace_id};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryAction {
    Continue,
    NotNow,
    Abandon,
}

impl fmt::Display for RecoveryAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Continue => "Continue",
            Self::NotNow => "Not now",
            Self::Abandon => "Abandon",
        })
    }
}

#[derive(Clone)]
struct IntentChoice(IntentSummary);

impl fmt::Display for IntentChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.objective)
    }
}

pub struct RecoveredIntent {
    pub session_id: String,
    pub spec: crate::intent::IntentSpec,
}

pub fn recover_session(config: &Config, auth: &AuthStore) -> Result<Option<RecoveredIntent>> {
    let workspace = workspace_id(&std::env::current_dir()?)?;
    let unfinished = list_unfinished(&config.intents_dir, &workspace)?;
    if unfinished.is_empty() {
        return Ok(None);
    }
    let selected = select_intent(unfinished)?;
    println!("\nUnfinished task\n{}\n", selected.objective);
    let action = inquire::Select::new(
        "Action:",
        vec![
            RecoveryAction::Continue,
            RecoveryAction::NotNow,
            RecoveryAction::Abandon,
        ],
    )
    .prompt()
    .map_err(|_| AppError::Cancelled("Intent recovery cancelled by user".to_string()))?;
    println!();
    match action {
        RecoveryAction::Continue => {
            let handle = IntentHandle::open(&config.intents_dir, &selected.intent_id, auth.secret_values())?;
            Ok(Some(RecoveredIntent {
                session_id: selected.session_id,
                spec: handle.snapshot()?.spec,
            }))
        }
        RecoveryAction::NotNow => Ok(None),
        RecoveryAction::Abandon => {
            IntentHandle::open(&config.intents_dir, &selected.intent_id, auth.secret_values())?.abandon()?;
            Ok(None)
        }
    }
}

fn select_intent(intents: Vec<IntentSummary>) -> Result<IntentSummary> {
    if intents.len() == 1 {
        return intents
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Intent("No unfinished intent was selected".to_string()));
    }
    inquire::Select::new("Unfinished task:", intents.into_iter().map(IntentChoice).collect())
        .prompt()
        .map(|choice| choice.0)
        .map_err(|_| AppError::Cancelled("Intent recovery cancelled by user".to_string()))
}
