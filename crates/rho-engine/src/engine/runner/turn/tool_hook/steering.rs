pub const STEERING_SKIP_REASON: &str = "Tool execution cancelled due to user steering interrupt.";

pub fn format_steering_message(prompt: &str) -> String {
    format!(
        "[USER STEERING INTERRUPT]:\n{}\n(System: Please adjust your approach immediately according to the user's latest instruction.)",
        prompt.trim()
    )
}

pub fn format_steering_messages(prompts: &[String]) -> String {
    let combined = prompts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    format_steering_message(&combined)
}

pub fn attach_steering_to_output(output: &str, steering_text: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        steering_text.to_string()
    } else {
        format!("{trimmed}\n\n{steering_text}")
    }
}
