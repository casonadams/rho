use rho_harness_core::a2ui::{BoundString, ButtonVariant};

use super::A2uiModalState;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::a2ui::render::{ModalFocus, RenderCtx, RenderedTree, clamp_line, style_line, walk_tree};
use crate::ui::theme::Theme;

mod choice;
mod text;
mod viewport;

pub(in crate::ui::interactive::a2ui) use choice::{
    ChoicePickerRender, render_modal_checkbox, render_modal_choice_picker,
};
pub(in crate::ui::interactive::a2ui) use text::{TextFieldRender, render_modal_text_field};
use viewport::{adjust_cursor, window_around_focus};

impl A2uiModalState {
    pub fn desired_lines(&self, width: usize) -> usize {
        self.render_tree(width.max(1), &Theme::default(), false)
            .lines
            .len()
            .max(1)
    }

    pub fn render(
        &self,
        width: usize,
        max_lines: usize,
        theme: &Theme,
        focused: bool,
    ) -> (Vec<String>, CursorPosition, bool) {
        if width == 0 || max_lines == 0 {
            return (Vec::new(), CursorPosition { row: 0, column: 0 }, false);
        }
        let tree = self.render_tree(width, theme, focused);
        let lines: Vec<String> = tree.lines.into_iter().map(|l| clamp_line(&l, width)).collect();
        let (windowed, offset) = window_around_focus(lines, tree.focus_range, max_lines, width, theme);
        let (mut cursor, cursor_visible) = adjust_cursor(tree.cursor, offset, windowed.len(), focused);
        let indented = windowed.into_iter().map(|l| format!("  {l}")).collect();
        if cursor_visible {
            cursor.column += 2;
        }
        (indented, cursor, cursor_visible)
    }

    fn render_tree(&self, width: usize, theme: &Theme, focused: bool) -> RenderedTree {
        walk_tree(&self.surface, width, theme, Some(ModalFocus { state: self, focused }))
    }
}

pub(in crate::ui::interactive::a2ui) fn render_modal_button(
    ctx: &mut RenderCtx<'_, '_>,
    modal: ModalFocus<'_>,
    id: &str,
    label: &BoundString,
    variant: ButtonVariant,
) -> Vec<String> {
    let is_focused = modal.state.focused_component_id() == Some(id);
    let resolved = ctx.surface.resolve_string(label);
    let style = match (is_focused, variant) {
        (true, ButtonVariant::Danger) => ctx.theme.tool_err.bold(),
        (true, _) => ctx.theme.highlight.bold(),
        (false, ButtonVariant::Primary) => ctx.theme.highlight,
        (false, ButtonVariant::Secondary) => ctx.theme.dimmed,
        (false, ButtonVariant::Danger) => ctx.theme.tool_err,
    };
    let text = if is_focused {
        format!("▸ [ {resolved} ]")
    } else {
        format!("[ {resolved} ]")
    };
    if is_focused {
        ctx.focus_range = Some((0, 1));
    }
    vec![clamp_line(&style_line(&text, style), ctx.width)]
}

#[cfg(test)]
mod tests {
    use super::super::super::test_support::{create_surface, key, single_select_request};
    use super::super::A2uiModalState;
    use crate::ui::block::{strip_ansi, visible_width};
    use crate::ui::theme::Theme;
    use crossterm::event::KeyCode;
    use rho_harness_core::a2ui::{
        A2uiComponent, A2uiComponentKind, A2uiMessage, A2uiModalRequest, BoundString, ChoiceOption, UpdateComponents,
    };

    #[test]
    fn masked_text_field_shows_every_character_in_modal() {
        let req = A2uiModalRequest {
            surface_id: "pw".to_string(),
            title: None,
            messages: vec![
                create_surface("pw", "field", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "pw".to_string(),
                    components: vec![A2uiComponent::new(
                        "field",
                        A2uiComponentKind::TextField {
                            label: None,
                            placeholder: None,
                            value_path: "/pw".to_string(),
                            multiline: false,
                            masked: true,
                        },
                    )],
                }),
            ],
            timeout_ms: None,
        };
        let mut state = A2uiModalState::from_request(req).unwrap();
        for _ in 0..20 {
            state.handle_key(key(KeyCode::Char('x')));
        }
        let (lines, _, _) = state.render(60, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert!(plain[0].contains(&"*".repeat(20)), "got: {plain:?}");
    }

    #[test]
    fn other_option_renders_ellipsis_when_idle_and_box_when_active() {
        let mut state = A2uiModalState::from_request(single_select_request(true)).unwrap();
        let (lines, _, _) = state.render(60, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let other_line = plain.iter().find(|l| l.contains("Other")).expect("other line");
        assert!(other_line.contains("○ Other…"), "got: {other_line}");
        assert!(!other_line.contains("Other: []"));

        state.handle_key(key(KeyCode::Char('3')));
        let (lines, _, cursor_vis) = state.render(60, 10, &Theme::default(), true);
        assert!(cursor_vis);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let other_line = plain.iter().find(|l| l.contains("Other")).expect("other line");
        assert!(other_line.contains("Other: ["), "got: {other_line}");
    }

    #[test]
    fn choice_descriptions_align_vertically_and_truncate_with_ellipsis() {
        let req = A2uiModalRequest {
            surface_id: "align".to_string(),
            title: Some("Align".to_string()),
            messages: vec![
                create_surface("align", "q1", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "align".to_string(),
                    components: vec![A2uiComponent::new(
                        "q1",
                        A2uiComponentKind::ChoicePicker {
                            label: None,
                            options: vec![
                                ChoiceOption {
                                    value: "0".to_string(),
                                    label: BoundString::literal("Go"),
                                    description: Some(BoundString::literal("Short desc")),
                                },
                                ChoiceOption {
                                    value: "1".to_string(),
                                    label: BoundString::literal("PostgreSQL"),
                                    description: Some(BoundString::literal(
                                        "Very long description that should truncate cleanly",
                                    )),
                                },
                            ],
                            selection_path: "/q1".to_string(),
                            multi_select: false,
                            allow_other: false,
                            other_path: None,
                        },
                    )],
                }),
            ],
            timeout_ms: None,
        };
        let state = A2uiModalState::from_request(req).unwrap();
        let (lines, _, _) = state.render(44, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let col0 = plain[0].find("Short desc").expect("first desc");
        let col1 = plain[1].find("Very long").expect("second desc");
        assert_eq!(
            visible_width(&plain[0][..col0]),
            visible_width(&plain[1][..col1]),
            "descriptions should align vertically: {plain:?}"
        );
        assert!(plain[1].ends_with('…'), "expected ellipsis truncation: {:?}", plain[1]);
    }
}
