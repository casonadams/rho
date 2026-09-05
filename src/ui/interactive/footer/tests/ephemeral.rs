use super::*;

#[test]
fn top_line_prefers_ephemeral_message_over_quota() {
    let footer = FooterState {
        activity: Activity::Idle,
        cwd: Some("/Users/alice/project".into()),
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80, Some("Model: gpt-4o (openai)"));
    assert!(line.ends_with("Model: gpt-4o (openai)"));
    assert!(!line.contains("5h: 80%"));
}

#[test]
fn top_line_returns_to_quota_after_message_clears() {
    let footer = FooterState {
        activity: Activity::Idle,
        cwd: Some("/work".into()),
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    assert!(format_top_line(&footer, 80, None).ends_with("5h: 80%"));
}

#[test]
fn ephemeral_message_flattens_newlines_to_single_slot() {
    let footer = FooterState {
        activity: Activity::Idle,
        ..FooterState::default()
    };
    let line = format_top_line(&footer, 80, Some("Steering queued\nat tool boundary"));
    assert!(line.contains("Steering queued at tool boundary"));
    assert!(!line.contains('\n'));
}

#[test]
fn blank_message_falls_back_to_persistent_status() {
    let footer = FooterState {
        activity: Activity::Idle,
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    assert!(format_top_line(&footer, 80, Some("   ")).ends_with("5h: 80%"));
}
