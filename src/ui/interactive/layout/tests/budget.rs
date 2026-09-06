use crate::ui::interactive::layout::budget::{MAX_MODAL_HEIGHT_RATIO, NormalBudgetInput, compute_normal_budget};

fn sample_budget_input(terminal_height: usize, total_editor_lines: usize, is_modal: bool) -> NormalBudgetInput {
    NormalBudgetInput {
        terminal_height,
        raw_queued_count: 0,
        raw_widgets_count: 0,
        raw_footer_count: 1,
        total_editor_lines,
        autocomplete_desired: 0,
        is_modal,
    }
}

#[test]
fn test_max_modal_height_ratio_constant() {
    assert_eq!(MAX_MODAL_HEIGHT_RATIO, 0.66);
}

#[test]
fn test_modal_budget_caps_at_66_percent_terminal_height() {
    let input = sample_budget_input(30, 50, true);
    let budget = compute_normal_budget(&input);
    let expected_cap = (30.0 * MAX_MODAL_HEIGHT_RATIO).round() as usize;
    assert_eq!(budget.editor_max_lines, expected_cap);
}

#[test]
fn test_non_modal_budget_does_not_apply_66_percent_cap() {
    let input = sample_budget_input(30, 50, false);
    let budget = compute_normal_budget(&input);
    let modal_cap = (30.0 * MAX_MODAL_HEIGHT_RATIO).round() as usize;
    assert!(budget.editor_max_lines > modal_cap);
}

#[test]
fn test_modal_budget_scales_with_content_below_cap() {
    let input = sample_budget_input(30, 6, true);
    let budget = compute_normal_budget(&input);
    assert_eq!(budget.editor_max_lines, 6);
}

#[test]
fn test_modal_budget_minimum_allocation_on_tiny_terminal() {
    for height in [1, 2, 4, 8] {
        let input = sample_budget_input(height, 20, true);
        let budget = compute_normal_budget(&input);
        assert!(budget.editor_max_lines >= 1);
    }
}
