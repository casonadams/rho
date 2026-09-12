use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState, QueueKind, QueuedMessage};
use unicode_width::UnicodeWidthStr;

#[test]
fn footer_contains_available_status_and_queue_count() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Thinking,
        model: "model".into(),
        context: Some("42% context".into()),
        quota: Some("80% quota".into()),
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    });

    assert!(layout.footer_lines[0].ends_with("80% quota"));
    assert!(layout.footer_lines[1].ends_with("model"));
}

fn test_status_layout(
    footer: &FooterState,
    (queued, widgets): (&[QueuedMessage], &[String]),
    width: usize,
) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: None,
        autocomplete: None,
        footer,
        system_message: None,
        queued_messages: queued,
        widget_lines: widgets,
        terminal_width: width,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    })
}

#[test]
fn queued_messages_render_above_the_working_line() {
    let footer = FooterState {
        activity: Activity::Working,
        model: "model".into(),
        ..FooterState::default()
    };
    let queued = [
        QueuedMessage {
            text: "first steer".into(),
            kind: QueueKind::Steering,
        },
        QueuedMessage {
            text: "next follow".into(),
            kind: QueueKind::FollowUp,
        },
    ];
    let layout = test_status_layout(&footer, (&queued, &[]), 80);

    assert_eq!(layout.queued_lines.len(), 3);
    assert!(
        layout.queued_lines[0].contains("Steering: first steer")
            && layout.queued_lines[1].contains("Follow-up: next follow")
    );
    assert!(layout.queued_lines[2].contains("Alt+↑") && layout.height() == 9);
}

#[test]
fn narrow_layout_never_exceeds_terminal_width() {
    let footer = FooterState {
        activity: Activity::Working,
        model: "model".into(),
        ..FooterState::default()
    };
    let layout = test_status_layout(&footer, (&[], &[]), 5);

    assert!(layout.footer_lines[0].width() <= 5 && layout.footer_lines[1].width() <= 5);
    assert_eq!(
        crate::ui::interactive::layout::text::visible_width(&layout.top_divider),
        5
    );
}

#[test]
fn queued_messages_render_below_widget_lines_and_above_editor() {
    let footer = FooterState {
        activity: Activity::Working,
        ..FooterState::default()
    };
    let queued = [QueuedMessage {
        text: "do this next".into(),
        kind: QueueKind::Steering,
    }];
    let widgets = vec![
        "┌─ bash cargo test ──┐".to_string(),
        "│ running 1 test     │".to_string(),
        "└────────────────────┘".to_string(),
    ];
    let layout = test_status_layout(&footer, (&queued, &widgets), 80);

    let widget_pos = layout.lines.iter().position(|l| l.contains("bash cargo test")).unwrap();
    let steering_pos = layout
        .lines
        .iter()
        .position(|l| l.contains("Steering: do this next"))
        .unwrap();
    let top_div_pos = layout.lines.iter().position(|l| l == &layout.top_divider).unwrap();
    assert!(widget_pos < steering_pos && steering_pos < top_div_pos);
}
