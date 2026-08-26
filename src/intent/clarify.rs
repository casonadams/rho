use crate::error::{AppError, Result};
use crate::intent::analyzer::{AmbiguityAnalysis, IntentAnalyzer};
use crate::intent::model::IntentSpec;

pub struct ClarificationHandler;

fn apply_clarification(spec: &mut IntentSpec, header: &str, answer: &str) {
    let decision = format!("{header}: {}", answer.trim());
    spec.constraints.push(decision.clone());
    spec.outcomes.push(format!("Honor the resolved decision: {decision}"));
}

impl ClarificationHandler {
    pub fn clarify_interactive(mut spec: IntentSpec, analysis: &AmbiguityAnalysis) -> Result<IntentSpec> {
        if !analysis.needs_clarification || analysis.questions.is_empty() {
            return Ok(spec);
        }

        let total = analysis.questions.len();
        println!("\nClarification\n");
        for (index, q) in analysis.questions.iter().enumerate() {
            let mut labels: Vec<String> = q.options.iter().map(|o| o.label.clone()).collect();
            if q.allow_custom {
                labels.push("Type something...".to_string());
            }

            println!("[{} {}/{}] {}\n", q.header, index + 1, total, q.question);
            let choice = inquire::Select::new("Answer:", labels)
                .prompt()
                .map_err(|_| AppError::Cancelled("clarification cancelled by user".to_string()))?;
            println!();

            let answer = if choice == "Type something..." {
                let custom = inquire::Text::new("Your answer:")
                    .prompt()
                    .map_err(|_| AppError::Cancelled("clarification cancelled by user".to_string()))?;
                println!();
                custom
            } else {
                choice
            };
            apply_clarification(&mut spec, &q.header, &answer);
        }
        spec.status = "clarified".to_string();
        println!();

        Ok(spec)
    }

    pub fn process_intent(prompt: &str, is_interactive: bool) -> Result<IntentSpec> {
        let mut spec = IntentSpec::from_prompt(prompt);
        let analysis = IntentAnalyzer::analyze(&spec);
        if analysis.detected_topics.iter().any(|topic| topic == "informational") {
            spec.status = "informational".to_string();
            return Ok(spec);
        }

        if is_interactive && analysis.needs_clarification {
            spec = Self::clarify_interactive(spec, &analysis)?;
        }

        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn informational_prompt_is_not_a_tracked_task() {
        let spec = ClarificationHandler::process_intent("what does this repo do?", true).unwrap();

        assert_eq!(spec.status, "informational");
    }

    #[test]
    fn clarification_becomes_a_binding_constraint_and_outcome() {
        let mut spec = IntentSpec::from_prompt("add authentication");

        apply_clarification(&mut spec, "Auth Strategy", "Session Cookie Auth");

        assert_eq!(spec.constraints, ["Auth Strategy: Session Cookie Auth"]);
        assert_eq!(
            spec.outcomes,
            ["Honor the resolved decision: Auth Strategy: Session Cookie Auth"]
        );
    }
}
