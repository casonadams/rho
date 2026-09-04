use crate::repl::interactive::Completion;
use crate::ui::interactive::layout::autocomplete::{MAX_VISIBLE_ITEMS, render_autocomplete_dropdown};
use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::state::autocomplete::AutocompleteState;
use crate::ui::interactive::{EditorState, FooterState};
use std::ops::Range;

fn make_items(count: usize) -> Vec<Completion> {
    (0..count)
        .map(|i| Completion {
            value: format!("/item{i}"),
            description: Some(format!("Description for item {i}")),
            replacement: Range { start: 0, end: 1 },
        })
        .collect()
}

#[test]
fn test_render_autocomplete_dropdown() {
    let mut state = AutocompleteState::default();
    let items = vec![
        Completion {
            value: "/model".to_string(),
            description: Some("Switch model".to_string()),
            replacement: Range { start: 0, end: 1 },
        },
        Completion {
            value: "/skill".to_string(),
            description: Some("Inspect skills".to_string()),
            replacement: Range { start: 0, end: 1 },
        },
    ];
    state.open(items);

    let lines = render_autocomplete_dropdown(&state, 60, MAX_VISIBLE_ITEMS);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("/model"));
    assert!(lines[0].contains("Switch model"));
    assert!(lines[1].contains("/skill"));
}

#[test]
fn descriptions_share_the_footer_dim_style() {
    let footer_dim = crate::ui::theme::Theme::default().dimmed.render().to_string();
    let mut state = AutocompleteState::default();
    state.open(vec![Completion {
        value: "/model".to_string(),
        description: Some("Switch model".to_string()),
        replacement: Range { start: 0, end: 1 },
    }]);

    let lines = render_autocomplete_dropdown(&state, 60, MAX_VISIBLE_ITEMS);
    assert!(lines[0].contains(&footer_dim), "{}", lines[0]);
    assert!(!lines[0].contains("\x1b[2;90m"), "{}", lines[0]);
}

#[test]
fn autocomplete_suppressed_when_fewer_than_two_rows_available() {
    let mut state = AutocompleteState::default();
    state.open(make_items(5));

    assert!(render_autocomplete_dropdown(&state, 60, 1).is_empty());
    assert!(render_autocomplete_dropdown(&state, 60, 0).is_empty());
}

#[test]
fn autocomplete_scales_down_to_max_lines() {
    let mut state = AutocompleteState::default();
    state.open(make_items(10));

    let lines = render_autocomplete_dropdown(&state, 60, 3);
    assert_eq!(lines.len(), 3);
}

#[test]
fn autocomplete_window_keeps_selection_in_view_when_scaled() {
    let mut state = AutocompleteState::default();
    state.open(make_items(10));

    state.selected = 0;
    let lines = render_autocomplete_dropdown(&state, 60, 4);
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("/item0"));

    state.selected = 5;
    let lines = render_autocomplete_dropdown(&state, 60, 4);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().any(|l| l.contains("/item5")));

    state.selected = 9;
    let lines = render_autocomplete_dropdown(&state, 60, 4);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().any(|l| l.contains("/item9")));
}

#[test]
fn autocomplete_layout_bounded_by_terminal_height() {
    let mut editor = EditorState::default();
    editor.set_text("/");
    let mut ac = AutocompleteState::default();
    ac.open(make_items(10));
    let footer = FooterState::default();

    let layout_8 = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: Some(&ac),
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 8,
        spinner_frame: 0,
        theme: None,
    });
    assert!(layout_8.height() <= 8);
    assert!(layout_8.lines.iter().any(|l| l.contains("/item")));

    let layout_6 = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: Some(&ac),
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 6,
        spinner_frame: 0,
        theme: None,
    });
    assert!(layout_6.height() <= 6);
    assert!(!layout_6.lines.iter().any(|l| l.contains("/item")));
}
