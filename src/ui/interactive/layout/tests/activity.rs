use super::base_input;
use crate::ui::interactive::layout::{InteractiveLayout, LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState, ModalOption, ModalState};

fn busy_footer(activity: Activity) -> FooterState {
    FooterState {
        activity,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    }
}

fn test_layout(footer: &FooterState, modal: Option<&ModalState>, focused: bool) -> InteractiveLayout {
    layout(LayoutInput {
        modal,
        focused,
        ..base_input(&EditorState::default(), footer)
    })
}

fn permission_modal() -> ModalState {
    ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![ModalOption::from("Allow")],
    )
}

fn assert_modal_hides_working(layout: &InteractiveLayout) {
    assert!(layout.top_divider.contains("Permission Required"));
    assert!(!layout.top_divider.contains("working"));
    assert!(!layout.lines.iter().any(|l| l.contains("working")));
}

#[test]
fn busy_activity_renders_working_line_above_the_editor() {
    let layout = test_layout(&busy_footer(Activity::Working), None, true);

    assert!(
        layout.working_line.starts_with(" \x1b[36m\u{280b}")
            && layout.working_line.contains("working")
            && layout.working_line.contains("\u{1b}[2m")
    );
    assert!(layout.footer_lines[1].ends_with("model/off"));
    assert!(layout.top_divider.contains("working"));
    assert!(layout.top_divider.contains('\u{280b}'));
    assert_eq!(layout.height(), 6);
}

#[test]
fn thinking_and_compacting_activities_render_their_labels() {
    for (activity, label, absent) in [
        (Activity::Thinking, "working", None),
        (Activity::Compacting, "compacting", Some("working")),
    ] {
        let layout = test_layout(&busy_footer(activity), None, true);

        assert!(layout.working_line.contains('\u{280b}'));
        assert!(layout.working_line.contains(label));
        assert!(absent.is_none_or(|word| !layout.working_line.contains(word)));
    }
}

#[test]
fn idle_activity_renders_no_working_line() {
    let layout = test_layout(&busy_footer(Activity::Idle), None, true);

    assert_eq!(layout.working_line, "");
    assert_eq!(layout.height(), 6);
}

#[test]
fn busy_activity_does_not_change_layout_height_or_cursor_row() {
    let idle = test_layout(&FooterState::default(), None, true);
    let busy = test_layout(&busy_footer(Activity::Working), None, true);

    assert_eq!(busy.height(), idle.height());
    assert_eq!(busy.cursor_row(), idle.cursor_row());
    assert_eq!(busy.editor_lines, idle.editor_lines);
    assert!(!busy.working_line.is_empty());
}

#[test]
fn busy_activity_under_modal_suppresses_spinner_and_shows_modal_title() {
    let modal = permission_modal();
    let layout = test_layout(&busy_footer(Activity::Working), Some(&modal), true);

    assert_modal_hides_working(&layout);
}

#[test]
fn busy_activity_retains_working_line_when_unfocused() {
    let layout = test_layout(&busy_footer(Activity::Working), None, false);

    assert!(!layout.working_line.is_empty());
    assert!(layout.working_line.contains("working"));
    assert!(layout.lines.iter().any(|l| l.contains("working")));
    assert!(!layout.cursor_visible);
}

#[test]
fn busy_activity_under_modal_suppresses_spinner_when_unfocused() {
    let modal = permission_modal();
    let layout = test_layout(&busy_footer(Activity::Working), Some(&modal), false);

    assert_modal_hides_working(&layout);
    assert!(!layout.cursor_visible);
}
