use std::collections::BTreeMap;

use rho_harness_core::a2ui::{A2uiComponentKind, A2uiSurface, VisitAction, resolve_pointer};
use serde_json::Value;

pub(super) const OTHER_SENTINEL: &str = "__other__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlKind {
    ChoicePicker(ChoicePicker),
    TextField { value_path: String, masked: bool },
    CheckBox { checked_path: String },
    Button { action_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoicePicker {
    pub option_values: Vec<String>,
    pub multi_select: bool,
    pub allow_other: bool,
    pub selection_path: String,
    pub other_path: Option<String>,
}

impl ChoicePicker {
    pub fn option_count(&self) -> usize {
        self.option_values.len() + usize::from(self.allow_other)
    }

    pub fn other_index(&self) -> Option<usize> {
        self.allow_other.then_some(self.option_values.len())
    }

    pub fn value_at(&self, cursor: usize) -> Option<&str> {
        if self.other_index() == Some(cursor) {
            return Some(OTHER_SENTINEL);
        }
        self.option_values.get(cursor).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusableControl {
    pub component_id: String,
    pub kind: ControlKind,
}

pub(super) fn collect_controls(surface: &A2uiSurface) -> Vec<FocusableControl> {
    let mut controls = Vec::new();
    surface.walk_tree(|component, _| {
        if let Some(kind) = control_kind_for(&component.kind) {
            controls.push(FocusableControl {
                component_id: component.id.clone(),
                kind,
            });
        }
        VisitAction::Continue
    });
    controls
}

fn control_kind_for(kind: &A2uiComponentKind) -> Option<ControlKind> {
    match kind {
        A2uiComponentKind::ChoicePicker {
            options,
            selection_path,
            multi_select,
            allow_other,
            other_path,
            ..
        } => Some(ControlKind::ChoicePicker(ChoicePicker {
            option_values: options.iter().map(|o| o.value.clone()).collect(),
            multi_select: *multi_select,
            allow_other: *allow_other,
            selection_path: selection_path.clone(),
            other_path: other_path.clone(),
        })),
        A2uiComponentKind::TextField { value_path, masked, .. } => Some(ControlKind::TextField {
            value_path: value_path.clone(),
            masked: *masked,
        }),
        A2uiComponentKind::CheckBox { checked_path, .. } => Some(ControlKind::CheckBox {
            checked_path: checked_path.clone(),
        }),
        A2uiComponentKind::Button { action, .. } => Some(ControlKind::Button {
            action_name: action.name.clone(),
        }),
        _ => None,
    }
}

pub(super) fn initial_choice_cursors(surface: &A2uiSurface, controls: &[FocusableControl]) -> BTreeMap<String, usize> {
    let mut cursors = BTreeMap::new();
    for ctrl in controls {
        let ControlKind::ChoicePicker(picker) = &ctrl.kind else {
            continue;
        };
        if picker.multi_select {
            continue;
        }
        if let Some(selected) = resolve_pointer(&surface.data_model, &picker.selection_path).and_then(Value::as_str)
            && let Some(idx) = picker.option_values.iter().position(|v| v == selected)
        {
            cursors.insert(ctrl.component_id.clone(), idx);
        }
    }
    cursors
}
