use crate::error::Result;
use crate::intent::analyzer::{AmbiguityAnalysis, IntentAnalyzer};
use crate::intent::model::IntentSpec;

pub struct ClarificationHandler;

impl ClarificationHandler {
    pub fn clarify_interactive(mut spec: IntentSpec, analysis: &AmbiguityAnalysis) -> Result<IntentSpec> {
        if !analysis.needs_clarification || analysis.questions.is_empty() {
            return Ok(spec);
        }

        println!("\n  [Clarification needed before proceeding]");
        for q in &analysis.questions {
            let mut labels: Vec<String> = q.options.iter().map(|o| o.label.clone()).collect();
            if q.allow_custom {
                labels.push("Custom / Type something...".to_string());
            }

            let prompt_text = format!("{} ({})", q.question, q.header);
            let ans = inquire::Select::new(&prompt_text, labels).prompt();

            match ans {
                Ok(choice) => {
                    let final_answer = if choice.starts_with("Custom") {
                        inquire::Text::new("Enter custom choice:").prompt().unwrap_or(choice)
                    } else {
                        choice
                    };

                    spec.constraints.push(format!("{}: {}", q.header, final_answer));
                    spec.outcomes.push(format!(
                        "Configured {} according to user choice: {}",
                        q.header, final_answer
                    ));
                }
                Err(_) => {
                    // User canceled / escaped, proceed with recommended default
                    if let Some(first) = q.options.first() {
                        spec.constraints.push(format!("{}: {}", q.header, first.label));
                    }
                }
            }
        }
        println!();

        Ok(spec)
    }

    pub fn process_intent(prompt: &str, is_interactive: bool) -> Result<IntentSpec> {
        let mut spec = IntentSpec::from_prompt(prompt);
        let analysis = IntentAnalyzer::analyze(&spec);

        if is_interactive && analysis.needs_clarification {
            spec = Self::clarify_interactive(spec, &analysis)?;
        }

        Ok(spec)
    }
}
