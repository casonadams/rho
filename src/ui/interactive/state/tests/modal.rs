use super::super::{InteractiveState, ModalOption, ModalState, QueueKind, UiAction};

#[test]
fn nested_modals_restore_each_saved_draft_without_changing_queue() {
    let mut state = InteractiveState::default();
    state.editor_mut().set_text("original draft");
    state.apply(UiAction::Submit(QueueKind::Steering));
    state.editor_mut().set_text("next draft");
    state.push_modal(ModalState::new("Approval", "Allow tool?", Vec::<ModalOption>::new()));
    state.editor_mut().set_text("modal response");
    state.push_modal(ModalState::new("Question", "Choose", vec![ModalOption::from("One")]));
    state.editor_mut().set_text("custom answer");

    assert_eq!(state.active_modal().unwrap().title, "Question");
    state.pop_modal();
    assert_eq!(state.editor().text(), "modal response");
    state.pop_modal();
    assert_eq!(state.editor().text(), "next draft");
    assert_eq!(state.queue_len(), 1);
}

#[test]
fn modal_filter_fuzzy_matches_subsequences_ranked() {
    let mut modal = ModalState::new(
        "Select Model",
        "",
        vec![
            ModalOption::new("gemini-2.5-flash", Some("[antigravity]")),
            ModalOption::new("gemini-3.8-flash", Some("[antigravity]")),
            ModalOption::new("gemini-3.1-pro", Some("[antigravity]")),
            ModalOption::new("claude-sonnet-4-6", Some("[antigravity]")),
        ],
    )
    .with_search(true);

    // Substring queries still work.
    modal.set_filter("gemin");
    assert_eq!(modal.options.len(), 3);

    // Subsequence queries match ("gem3" → gemini-3.x) and rank those first;
    // gemini-2.5 does not contain a '3' after 'm' so it drops out.
    modal.set_filter("gem3");
    let ids: Vec<&str> = modal.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(ids, vec!["gemini-3.8-flash", "gemini-3.1-pro"]);

    // Label matches outrank description matches at equal scores.
    modal.set_filter("claude");
    assert_eq!(modal.options[0].label, "claude-sonnet-4-6");

    modal.set_filter("");
    assert_eq!(modal.options.len(), 4);
}
