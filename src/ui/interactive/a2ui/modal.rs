use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::a2ui::{
    A2uiComponentKind, A2uiError, A2uiModalRequest, A2uiModalResult, A2uiSurface, A2uiUserAction, ActionBinding,
    SurfaceStore, apply_data_patch, resolve_pointer, resolve_typed,
};
use serde_json::{Map, Value};

use crate::ui::interactive::input::is_ctrl_c;

use controls::{ChoicePicker, ControlKind, FocusableControl, OTHER_SENTINEL, collect_controls, initial_choice_cursors};

mod controls;
mod options;
pub(super) mod view;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A2uiKeyOutcome {
    Handled,
    Completed(A2uiModalResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2uiModalState {
    surface_id: String,
    title: String,
    surface: A2uiSurface,
    controls: Vec<FocusableControl>,
    focused_index: usize,
    choice_cursors: BTreeMap<String, usize>,
    editing_other: bool,
    warnings: Vec<String>,
}

impl A2uiModalState {
    pub fn from_request(request: A2uiModalRequest) -> Result<Self, A2uiError> {
        let mut store = SurfaceStore::new();
        store.apply_all(request.messages)?;
        let surface = store
            .surface(&request.surface_id)
            .ok_or_else(|| A2uiError::SurfaceNotFound {
                surface_id: request.surface_id.clone(),
            })?
            .clone();
        let title = request
            .title
            .or_else(|| surface.title.clone())
            .unwrap_or_else(|| "Clarification".to_string());
        let controls = collect_controls(&surface);
        let choice_cursors = initial_choice_cursors(&surface, &controls);
        Ok(Self {
            surface_id: request.surface_id,
            title,
            surface,
            controls,
            focused_index: 0,
            choice_cursors,
            editing_other: false,
            warnings: Vec::new(),
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn surface(&self) -> &A2uiSurface {
        &self.surface
    }

    pub fn focused_control(&self) -> Option<&FocusableControl> {
        self.controls.get(self.focused_index)
    }

    pub fn focused_component_id(&self) -> Option<&str> {
        self.focused_control().map(|c| c.component_id.as_str())
    }

    pub fn is_typing_text(&self) -> bool {
        match self.focused_control().map(|c| &c.kind) {
            Some(ControlKind::TextField { .. }) => true,
            Some(ControlKind::ChoicePicker(_)) => self.editing_other,
            _ => false,
        }
    }

    pub fn footer_hint(&self) -> &'static str {
        if self.is_typing_text() {
            return "Type answer • Tab next • Enter confirm • Ctrl+C clear • Esc cancel";
        }
        match self.focused_control().map(|c| &c.kind) {
            Some(ControlKind::ChoicePicker(ChoicePicker { multi_select: true, .. })) => {
                "↑/↓ select • Space toggle • 1-9 jump • Tab next • Enter confirm • Esc cancel"
            }
            Some(ControlKind::ChoicePicker(_)) => "↑/↓ select • 1-9 jump • Tab next • Enter confirm • Esc cancel",
            Some(ControlKind::CheckBox { .. }) => "Space/Enter toggle • Tab next • Esc cancel",
            Some(ControlKind::Button { .. }) => "Enter activate • Tab next • Esc cancel",
            Some(ControlKind::TextField { .. }) => "Type answer • Tab next • Enter confirm • Ctrl+C clear • Esc cancel",
            None => "Esc cancel",
        }
    }

    pub fn handle_paste(&mut self, text: &str) -> bool {
        let Some(path) = self.active_text_path().map(str::to_string) else {
            return false;
        };
        let cleaned = text.replace(['\r', '\n'], " ");
        let mut current = self.read_string(&path);
        current.push_str(&cleaned);
        self.write_value(&path, Value::String(current));
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> A2uiKeyOutcome {
        if key.code == KeyCode::Esc {
            return A2uiKeyOutcome::Completed(A2uiModalResult::Cancelled);
        }
        if key.code == KeyCode::Tab {
            self.focus_next();
            return A2uiKeyOutcome::Handled;
        }
        if key.code == KeyCode::BackTab {
            self.focus_prev();
            return A2uiKeyOutcome::Handled;
        }
        if is_ctrl_c(&key) {
            self.clear_active_text();
            return A2uiKeyOutcome::Handled;
        }
        if key.code == KeyCode::Backspace {
            self.pop_active_text();
            return A2uiKeyOutcome::Handled;
        }
        self.handle_non_global_key(key)
    }

    fn handle_non_global_key(&mut self, key: KeyEvent) -> A2uiKeyOutcome {
        match key.code {
            KeyCode::Up => {
                self.move_up();
                A2uiKeyOutcome::Handled
            }
            KeyCode::Down => {
                self.move_down();
                A2uiKeyOutcome::Handled
            }
            KeyCode::Left => {
                if self.is_button_focused() {
                    self.focus_prev();
                }
                A2uiKeyOutcome::Handled
            }
            KeyCode::Right => {
                if self.is_button_focused() {
                    self.focus_next();
                }
                A2uiKeyOutcome::Handled
            }
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Char(c) => self.handle_char(c, key.modifiers),
            _ => A2uiKeyOutcome::Handled,
        }
    }

    fn handle_char(&mut self, c: char, modifiers: KeyModifiers) -> A2uiKeyOutcome {
        if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return A2uiKeyOutcome::Handled;
        }
        if self.is_typing_text() {
            if let Some(path) = self.active_text_path().map(str::to_string) {
                let mut text = self.read_string(&path);
                text.push(c);
                self.write_value(&path, Value::String(text));
            }
            return A2uiKeyOutcome::Handled;
        }
        match c {
            'k' => self.move_up(),
            'j' => self.move_down(),
            ' ' => self.handle_space(),
            '1'..='9' => self.handle_digit((c as u8 - b'1') as usize),
            _ => {}
        }
        A2uiKeyOutcome::Handled
    }

    fn handle_space(&mut self) {
        let Some(ctrl) = self.focused_control().cloned() else {
            return;
        };
        match ctrl.kind {
            ControlKind::CheckBox { checked_path } => self.toggle_checkbox(&checked_path),
            ControlKind::ChoicePicker(picker) => {
                let cursor = self.choice_cursor(&ctrl.component_id);
                if picker.multi_select {
                    self.toggle_multi(&picker, cursor);
                } else {
                    self.select_single(&picker, cursor);
                }
            }
            _ => {}
        }
    }

    fn handle_digit(&mut self, idx: usize) {
        let Some(ctrl) = self.focused_control().cloned() else {
            return;
        };
        let ControlKind::ChoicePicker(picker) = ctrl.kind else {
            return;
        };
        if idx >= picker.option_count() {
            return;
        }
        self.choice_cursors.insert(ctrl.component_id, idx);
        if !picker.multi_select {
            self.select_single(&picker, idx);
        }
    }

    fn handle_enter(&mut self) -> A2uiKeyOutcome {
        let Some(ctrl) = self.focused_control().cloned() else {
            return A2uiKeyOutcome::Handled;
        };
        match ctrl.kind {
            ControlKind::Button { action_name } => self.activate_button(&ctrl.component_id, &action_name),
            ControlKind::CheckBox { checked_path } => {
                self.toggle_checkbox(&checked_path);
                A2uiKeyOutcome::Handled
            }
            ControlKind::TextField { .. } => self.advance_or_submit(),
            ControlKind::ChoicePicker(picker) => self.handle_choice_enter(&ctrl.component_id, &picker),
        }
    }

    fn handle_choice_enter(&mut self, component_id: &str, picker: &ChoicePicker) -> A2uiKeyOutcome {
        let cursor = self.choice_cursor(component_id);
        if picker.other_index() == Some(cursor) {
            if self.editing_other {
                self.editing_other = false;
                return self.advance_or_submit();
            }
            if picker.multi_select {
                self.ensure_multi_selected(&picker.selection_path, OTHER_SENTINEL);
            } else {
                self.select_single(picker, cursor);
            }
            self.editing_other = true;
            return A2uiKeyOutcome::Handled;
        }
        self.editing_other = false;
        if !picker.multi_select {
            self.select_single(picker, cursor);
        }
        self.advance_or_submit()
    }

    fn advance_or_submit(&mut self) -> A2uiKeyOutcome {
        if let Some(next_idx) = self.next_non_button_index() {
            self.focused_index = next_idx;
            self.editing_other = false;
            return A2uiKeyOutcome::Handled;
        }
        A2uiKeyOutcome::Completed(self.build_default_submitted())
    }

    fn next_non_button_index(&self) -> Option<usize> {
        self.controls
            .iter()
            .enumerate()
            .skip(self.focused_index + 1)
            .find(|(_, c)| !matches!(c.kind, ControlKind::Button { .. }))
            .map(|(i, _)| i)
    }

    fn activate_button(&self, component_id: &str, action_name: &str) -> A2uiKeyOutcome {
        if action_name == "cancel" {
            return A2uiKeyOutcome::Completed(A2uiModalResult::Cancelled);
        }
        A2uiKeyOutcome::Completed(self.build_submitted(component_id, action_name))
    }

    fn build_default_submitted(&self) -> A2uiModalResult {
        if let Some(btn) = self
            .controls
            .iter()
            .find(|c| matches!(&c.kind, ControlKind::Button { action_name } if action_name != "cancel"))
            && let ControlKind::Button { action_name } = &btn.kind
        {
            return self.build_submitted(&btn.component_id, action_name);
        }
        self.build_submitted("submit", "submit")
    }

    fn build_submitted(&self, component_id: &str, action_name: &str) -> A2uiModalResult {
        let context = self
            .surface
            .component(component_id)
            .and_then(|c| match &c.kind {
                A2uiComponentKind::Button { action, .. } => Some(resolve_action_context(&self.surface, action)),
                _ => None,
            })
            .unwrap_or_else(|| Value::Object(Map::new()));
        A2uiModalResult::Submitted {
            action: A2uiUserAction {
                surface_id: self.surface_id.clone(),
                action_name: action_name.to_string(),
                source_component_id: component_id.to_string(),
                context,
            },
            data_model: self.surface.data_model.clone(),
        }
    }

    fn focus_next(&mut self) {
        if !self.controls.is_empty() {
            self.focused_index = (self.focused_index + 1) % self.controls.len();
            self.editing_other = false;
        }
    }

    fn focus_prev(&mut self) {
        if !self.controls.is_empty() {
            self.focused_index = (self.focused_index + self.controls.len() - 1) % self.controls.len();
            self.editing_other = false;
        }
    }

    fn is_button_focused(&self) -> bool {
        matches!(
            self.focused_control().map(|c| &c.kind),
            Some(ControlKind::Button { .. })
        )
    }

    fn move_up(&mut self) {
        let Some(ctrl) = self.focused_control().cloned() else {
            return;
        };
        if let ControlKind::ChoicePicker(_) = ctrl.kind {
            self.editing_other = false;
            let cursor = self.choice_cursor(&ctrl.component_id);
            self.choice_cursors.insert(ctrl.component_id, cursor.saturating_sub(1));
        } else {
            self.focus_prev();
        }
    }

    fn move_down(&mut self) {
        let Some(ctrl) = self.focused_control().cloned() else {
            return;
        };
        if let ControlKind::ChoicePicker(picker) = ctrl.kind {
            self.editing_other = false;
            let total = picker.option_count();
            if total > 0 {
                let cursor = self.choice_cursor(&ctrl.component_id);
                self.choice_cursors
                    .insert(ctrl.component_id, (cursor + 1).min(total - 1));
            }
        } else {
            self.focus_next();
        }
    }

    fn choice_cursor(&self, component_id: &str) -> usize {
        self.choice_cursors.get(component_id).copied().unwrap_or(0)
    }

    fn select_single(&mut self, picker: &ChoicePicker, cursor: usize) {
        let Some(value) = picker.value_at(cursor) else {
            return;
        };
        self.write_value(&picker.selection_path, Value::String(value.to_string()));
        self.editing_other = picker.other_index() == Some(cursor);
    }

    fn toggle_multi(&mut self, picker: &ChoicePicker, cursor: usize) {
        let Some(target) = picker.value_at(cursor) else {
            return;
        };
        let mut items = self.read_string_array(&picker.selection_path);
        if let Some(pos) = items.iter().position(|x| x == target) {
            items.remove(pos);
            if target == OTHER_SENTINEL {
                self.editing_other = false;
            }
        } else {
            items.push(target.to_string());
            if target == OTHER_SENTINEL {
                self.editing_other = true;
            }
        }
        let arr = items.into_iter().map(Value::String).collect();
        self.write_value(&picker.selection_path, Value::Array(arr));
    }

    fn ensure_multi_selected(&mut self, selection_path: &str, value: &str) {
        let mut items = self.read_string_array(selection_path);
        if !items.iter().any(|x| x == value) {
            items.push(value.to_string());
            let arr = items.into_iter().map(Value::String).collect();
            self.write_value(selection_path, Value::Array(arr));
        }
    }

    fn toggle_checkbox(&mut self, checked_path: &str) {
        let current = resolve_pointer(&self.surface.data_model, checked_path)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.write_value(checked_path, Value::Bool(!current));
    }

    fn active_text_path(&self) -> Option<&str> {
        match self.focused_control().map(|c| &c.kind) {
            Some(ControlKind::TextField { value_path, .. }) => Some(value_path.as_str()),
            Some(ControlKind::ChoicePicker(ChoicePicker {
                selection_path,
                other_path,
                ..
            })) if self.editing_other => Some(other_path.as_deref().unwrap_or(selection_path.as_str())),
            _ => None,
        }
    }

    fn clear_active_text(&mut self) {
        if let Some(path) = self.active_text_path().map(str::to_string) {
            self.write_value(&path, Value::String(String::new()));
        }
    }

    fn pop_active_text(&mut self) {
        if let Some(path) = self.active_text_path().map(str::to_string) {
            let mut text = self.read_string(&path);
            text.pop();
            self.write_value(&path, Value::String(text));
        }
    }

    fn read_string(&self, path: &str) -> String {
        resolve_pointer(&self.surface.data_model, path)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    fn read_string_array(&self, path: &str) -> Vec<String> {
        resolve_pointer(&self.surface.data_model, path)
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn write_value(&mut self, path: &str, value: Value) {
        if let Err(err) = apply_data_patch(&mut self.surface.data_model, Some(path), value) {
            self.warnings.push(format!("could not update {path}: {err}"));
        }
    }

    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

fn resolve_action_context(surface: &A2uiSurface, action: &ActionBinding) -> Value {
    let mut map = Map::new();
    for (key, bound) in &action.context {
        let resolved = resolve_typed::<Value>(&surface.data_model, bound).unwrap_or(Value::Null);
        map.insert(key.clone(), resolved);
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{create_surface, ctrl_c, key, single_select_request};
    use super::*;
    use crate::ui::block::strip_ansi;
    use crate::ui::theme::Theme;
    use rho_harness_core::a2ui::{
        A2uiComponent, A2uiMessage, BoundString, ChoiceOption, UpdateComponents, UpdateDataModel,
    };
    use serde_json::json;

    #[test]
    fn single_select_navigation_digits_other_and_submit() {
        let mut state = A2uiModalState::from_request(single_select_request(true)).unwrap();
        assert_eq!(state.choice_cursor("q1"), 0);

        assert_eq!(state.handle_key(key(KeyCode::Down)), A2uiKeyOutcome::Handled);
        assert_eq!(state.choice_cursor("q1"), 1);
        assert_eq!(state.handle_key(key(KeyCode::Char('k'))), A2uiKeyOutcome::Handled);
        assert_eq!(state.choice_cursor("q1"), 0);
        assert_eq!(state.handle_key(key(KeyCode::Char('j'))), A2uiKeyOutcome::Handled);
        assert_eq!(state.choice_cursor("q1"), 1);

        // Digit jump '3' hits Other (index 2) and enters inline editing
        assert_eq!(state.handle_key(key(KeyCode::Char('3'))), A2uiKeyOutcome::Handled);
        assert!(state.is_typing_text());
        for ch in "DuckDB".chars() {
            assert_eq!(state.handle_key(key(KeyCode::Char(ch))), A2uiKeyOutcome::Handled);
        }
        let A2uiKeyOutcome::Completed(A2uiModalResult::Submitted { action, data_model }) =
            state.handle_key(key(KeyCode::Enter))
        else {
            panic!("expected Submitted");
        };
        assert_eq!(action.action_name, "submit");
        assert_eq!(data_model["answers"]["q1"], "__other__");
        assert_eq!(data_model["answers"]["q1_other"], "DuckDB");
    }

    #[test]
    fn multi_select_choice_picker_and_checkbox_toggle() {
        let req = A2uiModalRequest {
            surface_id: "form".to_string(),
            title: Some("Options".to_string()),
            messages: vec![
                create_surface("form", "root", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "form".to_string(),
                    components: vec![
                        A2uiComponent::new(
                            "multi",
                            A2uiComponentKind::ChoicePicker {
                                label: Some(BoundString::literal("Features")),
                                options: vec![
                                    ChoiceOption {
                                        value: "a".to_string(),
                                        label: BoundString::literal("Alpha"),
                                        description: None,
                                    },
                                    ChoiceOption {
                                        value: "b".to_string(),
                                        label: BoundString::literal("Beta"),
                                        description: None,
                                    },
                                ],
                                selection_path: "/features".to_string(),
                                multi_select: true,
                                allow_other: false,
                                other_path: None,
                            },
                        ),
                        A2uiComponent::new(
                            "agree",
                            A2uiComponentKind::CheckBox {
                                label: BoundString::literal("Confirm"),
                                checked_path: "/agree".to_string(),
                            },
                        ),
                        A2uiComponent::new(
                            "root",
                            A2uiComponentKind::Column {
                                children: vec!["multi".to_string(), "agree".to_string()],
                                gap: Some(1),
                            },
                        ),
                    ],
                }),
                A2uiMessage::UpdateDataModel(UpdateDataModel {
                    surface_id: "form".to_string(),
                    path: None,
                    value: json!({ "features": [], "agree": false }),
                }),
            ],
            timeout_ms: None,
        };
        let mut state = A2uiModalState::from_request(req).unwrap();
        assert_eq!(state.handle_key(key(KeyCode::Char(' '))), A2uiKeyOutcome::Handled);
        assert_eq!(state.handle_key(key(KeyCode::Down)), A2uiKeyOutcome::Handled);
        assert_eq!(state.handle_key(key(KeyCode::Char(' '))), A2uiKeyOutcome::Handled);
        assert_eq!(state.surface().data_model["features"], json!(["a", "b"]));

        // Toggle 'a' back off
        assert_eq!(state.handle_key(key(KeyCode::Up)), A2uiKeyOutcome::Handled);
        assert_eq!(state.handle_key(key(KeyCode::Char(' '))), A2uiKeyOutcome::Handled);
        assert_eq!(state.surface().data_model["features"], json!(["b"]));

        // Tab to CheckBox and toggle with Space and Enter
        assert_eq!(state.handle_key(key(KeyCode::Tab)), A2uiKeyOutcome::Handled);
        assert_eq!(state.focused_component_id(), Some("agree"));
        assert_eq!(state.handle_key(key(KeyCode::Char(' '))), A2uiKeyOutcome::Handled);
        assert_eq!(state.surface().data_model["agree"], json!(true));
        assert_eq!(state.handle_key(key(KeyCode::Enter)), A2uiKeyOutcome::Handled);
        assert_eq!(state.surface().data_model["agree"], json!(false));
    }

    #[test]
    fn text_field_typing_backspace_ctrl_c_and_masked_rendering() {
        let req = A2uiModalRequest {
            surface_id: "secret".to_string(),
            title: Some("Auth".to_string()),
            messages: vec![
                create_surface("secret", "token", None),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "secret".to_string(),
                    components: vec![A2uiComponent::new(
                        "token",
                        A2uiComponentKind::TextField {
                            label: Some(BoundString::literal("API Key")),
                            placeholder: Some("enter key".to_string()),
                            value_path: "/token".to_string(),
                            multiline: false,
                            masked: true,
                        },
                    )],
                }),
            ],
            timeout_ms: None,
        };
        let mut state = A2uiModalState::from_request(req).unwrap();
        for ch in "abc1".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        state.handle_key(key(KeyCode::Backspace));
        assert_eq!(state.surface().data_model["token"], json!("abc"));

        let (lines, cursor, cursor_vis) = state.render(60, 10, &Theme::default(), true);
        assert!(cursor_vis);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        assert_eq!(plain[0], "  API Key");
        assert!(plain[1].starts_with("  ▸ ["), "got: {:?}", plain);
        assert!(plain[1].contains("***"), "expected masked output, got: {:?}", plain);
        assert!(!plain[1].contains("abc"));
        assert_eq!(cursor.row, 1);

        // Ctrl+C clears the input buffer without cancelling
        assert_eq!(state.handle_key(ctrl_c()), A2uiKeyOutcome::Handled);
        assert_eq!(state.surface().data_model["token"], json!(""));
    }

    #[test]
    fn esc_and_cancel_button_return_cancelled() {
        let mut state = A2uiModalState::from_request(single_select_request(false)).unwrap();
        assert_eq!(
            state.handle_key(key(KeyCode::Esc)),
            A2uiKeyOutcome::Completed(A2uiModalResult::Cancelled)
        );

        let mut state = A2uiModalState::from_request(single_select_request(false)).unwrap();
        assert_eq!(state.handle_key(key(KeyCode::Tab)), A2uiKeyOutcome::Handled);
        assert_eq!(state.focused_component_id(), Some("cancel_btn"));
        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            A2uiKeyOutcome::Completed(A2uiModalResult::Cancelled)
        );
    }
}
