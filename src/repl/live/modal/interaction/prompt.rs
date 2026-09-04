pub fn is_input_trigger(label: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "with reason",
        "with feedback",
        "custom answer",
        "custom input",
        "Type something",
        "Type a custom",
        "Deny with reason",
        "Accept input",
    ];
    PATTERNS.iter().any(|p| label.contains(p))
}

pub fn prompt_label_for(label: &str) -> &'static str {
    if label.contains("reason")
        || label.contains("feedback")
        || label.contains("Permission")
        || label.contains("Approve")
    {
        "reason"
    } else {
        "answer"
    }
}
