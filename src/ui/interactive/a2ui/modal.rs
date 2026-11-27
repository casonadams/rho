use std::collections::{BTreeMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::a2ui::{
    A2uiComponent, A2uiComponentKind, A2uiModalRequest, A2uiModalResult, A2uiSurface, A2uiUserAction, ActionBinding,
    BoundString, ButtonVariant, ChoiceOption, MAX_DEPTH, SemanticTone, SurfaceStore, TextUsageHint, VisitAction,
    apply_data_patch, resolve_pointer, resolve_typed,
};
use serde_json::{Map, Value};

use super::render::{
    card_border_style, card_top_border, clamp_line, layout_row_segments, render_badge, render_code, render_divider,
    render_table, render_text, style_line,
};
use crate::ui::block::visible_width;
use crate::ui::interactive::CursorPosition;
use crate::ui::interactive::footer::truncate_to_width;
use crate::ui::theme::{CursorMode, Theme};

const OTHER_SENTINEL: &str = "__other__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A2uiKeyOutcome {
    Handled,
    Completed(A2uiModalResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlKind {
    ChoicePicker {
        option_values: Vec<String>,
        multi_select: bool,
        allow_other: bool,
        selection_path: String,
        other_path: Option<String>,
    },
    TextField {
        value_path: String,
        masked: bool,
    },
    CheckBox {
        checked_path: String,
    },
    Button {
        action_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusableControl {
    pub component_id: String,
    pub kind: ControlKind,
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
}

impl A2uiModalState {
    pub fn from_request(request: A2uiModalRequest) -> Option<Self> {
        let mut store = SurfaceStore::new();
        store.apply_all(request.messages).ok()?;
        let surface = store.surface(&request.surface_id)?.clone();
        let title = request
            .title
            .or_else(|| surface.title.clone())
            .unwrap_or_else(|| "Clarification".to_string());
        let controls = collect_controls(&surface);
        let choice_cursors = initial_choice_cursors(&surface, &controls);
        Some(Self {
            surface_id: request.surface_id,
            title,
            surface,
            controls,
            focused_index: 0,
            choice_cursors,
            editing_other: false,
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
            Some(ControlKind::ChoicePicker { .. }) => self.editing_other,
            _ => false,
        }
    }

    pub fn footer_hint(&self) -> &'static str {
        if self.is_typing_text() {
            return "Type answer • Tab next • Enter confirm • Ctrl+C clear • Esc cancel";
        }
        match self.focused_control().map(|c| &c.kind) {
            Some(ControlKind::ChoicePicker { multi_select: true, .. }) => {
                "↑/↓ select • Space toggle • 1-9 jump • Tab next • Enter confirm • Esc cancel"
            }
            Some(ControlKind::ChoicePicker { .. }) => "↑/↓ select • 1-9 jump • Tab next • Enter confirm • Esc cancel",
            Some(ControlKind::CheckBox { .. }) => "Space/Enter toggle • Tab next • Esc cancel",
            Some(ControlKind::Button { .. }) => "Enter activate • Tab next • Esc cancel",
            Some(ControlKind::TextField { .. }) => "Type answer • Tab next • Enter confirm • Ctrl+C clear • Esc cancel",
            None => "Esc cancel",
        }
    }

    pub fn desired_lines(&self, width: usize) -> usize {
        let theme = Theme::default();
        let mut visited = HashSet::new();
        let mut ctx = ModalRenderCtx {
            state: self,
            depth: 0,
            visited: &mut visited,
            width: width.max(1),
            theme: &theme,
            focused: false,
            cursor: None,
            focus_range: None,
        };
        let lines = render_modal_node(&mut ctx, &self.surface.root_id);
        lines.len().max(1)
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
        let mut visited = HashSet::new();
        let mut ctx = ModalRenderCtx {
            state: self,
            depth: 0,
            visited: &mut visited,
            width,
            theme,
            focused,
            cursor: None,
            focus_range: None,
        };
        let lines: Vec<String> = render_modal_node(&mut ctx, &self.surface.root_id)
            .into_iter()
            .map(|l| clamp_line(&l, width))
            .collect();
        let (windowed, offset) = window_around_focus(lines, ctx.focus_range, max_lines, width, theme);
        let (mut cursor, cursor_visible) = adjust_cursor(ctx.cursor, offset, windowed.len(), focused);
        let indented = windowed.into_iter().map(|l| format!("  {l}")).collect();
        if cursor_visible {
            cursor.column += 2;
        }
        (indented, cursor, cursor_visible)
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
        if is_ctrl_c(key) {
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
            ControlKind::ChoicePicker {
                option_values,
                multi_select,
                allow_other,
                selection_path,
                ..
            } => {
                let cursor = self.choice_cursor(&ctrl.component_id);
                if multi_select {
                    self.toggle_multi_choice(&option_values, allow_other, &selection_path, cursor);
                } else {
                    self.select_single_choice(&option_values, allow_other, &selection_path, cursor);
                }
            }
            _ => {}
        }
    }

    fn handle_digit(&mut self, idx: usize) {
        let Some(ctrl) = self.focused_control().cloned() else {
            return;
        };
        let ControlKind::ChoicePicker {
            option_values,
            multi_select,
            allow_other,
            selection_path,
            ..
        } = ctrl.kind
        else {
            return;
        };
        let total = option_values.len() + usize::from(allow_other);
        if idx >= total {
            return;
        }
        self.choice_cursors.insert(ctrl.component_id, idx);
        if !multi_select {
            self.select_single_choice(&option_values, allow_other, &selection_path, idx);
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
            ControlKind::ChoicePicker {
                option_values,
                multi_select,
                allow_other,
                selection_path,
                ..
            } => self.handle_choice_enter(
                &ctrl.component_id,
                &option_values,
                multi_select,
                allow_other,
                &selection_path,
            ),
        }
    }

    fn handle_choice_enter(
        &mut self,
        component_id: &str,
        option_values: &[String],
        multi_select: bool,
        allow_other: bool,
        selection_path: &str,
    ) -> A2uiKeyOutcome {
        let cursor = self.choice_cursor(component_id);
        let is_other = allow_other && cursor == option_values.len();
        if is_other {
            if !self.editing_other {
                if multi_select {
                    self.ensure_multi_selected(selection_path, OTHER_SENTINEL);
                } else {
                    self.write_value(selection_path, Value::String(OTHER_SENTINEL.to_string()));
                }
                self.editing_other = true;
                return A2uiKeyOutcome::Handled;
            }
            self.editing_other = false;
            return self.advance_or_submit();
        }
        self.editing_other = false;
        if !multi_select {
            self.select_single_choice(option_values, allow_other, selection_path, cursor);
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
        if let ControlKind::ChoicePicker { .. } = ctrl.kind {
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
        if let ControlKind::ChoicePicker {
            option_values,
            allow_other,
            ..
        } = ctrl.kind
        {
            self.editing_other = false;
            let total = option_values.len() + usize::from(allow_other);
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

    fn select_single_choice(
        &mut self,
        option_values: &[String],
        allow_other: bool,
        selection_path: &str,
        cursor: usize,
    ) {
        if allow_other && cursor == option_values.len() {
            self.write_value(selection_path, Value::String(OTHER_SENTINEL.to_string()));
            self.editing_other = true;
        } else if let Some(val) = option_values.get(cursor) {
            self.write_value(selection_path, Value::String(val.clone()));
            self.editing_other = false;
        }
    }

    fn toggle_multi_choice(
        &mut self,
        option_values: &[String],
        allow_other: bool,
        selection_path: &str,
        cursor: usize,
    ) {
        let target = if allow_other && cursor == option_values.len() {
            OTHER_SENTINEL.to_string()
        } else if let Some(val) = option_values.get(cursor) {
            val.clone()
        } else {
            return;
        };
        let mut items = self.read_string_array(selection_path);
        if let Some(pos) = items.iter().position(|x| x == &target) {
            items.remove(pos);
            if target == OTHER_SENTINEL {
                self.editing_other = false;
            }
        } else {
            items.push(target.clone());
            if target == OTHER_SENTINEL {
                self.editing_other = true;
            }
        }
        let arr = items.into_iter().map(Value::String).collect();
        self.write_value(selection_path, Value::Array(arr));
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
            Some(ControlKind::ChoicePicker {
                selection_path,
                other_path,
                ..
            }) if self.editing_other => Some(other_path.as_deref().unwrap_or(selection_path.as_str())),
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
        let _ = apply_data_patch(&mut self.surface.data_model, Some(path), value);
    }
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn resolve_action_context(surface: &A2uiSurface, action: &ActionBinding) -> Value {
    let mut map = Map::new();
    for (key, bound) in &action.context {
        let resolved = resolve_typed::<Value>(&surface.data_model, bound).unwrap_or(Value::Null);
        map.insert(key.clone(), resolved);
    }
    Value::Object(map)
}

fn collect_controls(surface: &A2uiSurface) -> Vec<FocusableControl> {
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
        } => Some(ControlKind::ChoicePicker {
            option_values: options.iter().map(|o| o.value.clone()).collect(),
            multi_select: *multi_select,
            allow_other: *allow_other,
            selection_path: selection_path.clone(),
            other_path: other_path.clone(),
        }),
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

fn initial_choice_cursors(surface: &A2uiSurface, controls: &[FocusableControl]) -> BTreeMap<String, usize> {
    let mut cursors = BTreeMap::new();
    for ctrl in controls {
        let ControlKind::ChoicePicker {
            option_values,
            multi_select: false,
            selection_path,
            ..
        } = &ctrl.kind
        else {
            continue;
        };
        if let Some(selected) = resolve_pointer(&surface.data_model, selection_path).and_then(Value::as_str)
            && let Some(idx) = option_values.iter().position(|v| v == selected)
        {
            cursors.insert(ctrl.component_id.clone(), idx);
        }
    }
    cursors
}

struct ModalRenderCtx<'a, 'b> {
    state: &'a A2uiModalState,
    depth: usize,
    visited: &'b mut HashSet<&'a str>,
    width: usize,
    theme: &'a Theme,
    focused: bool,
    cursor: Option<CursorPosition>,
    focus_range: Option<(usize, usize)>,
}

impl<'a> ModalRenderCtx<'a, '_> {
    fn child(&mut self, width: usize) -> ModalRenderCtx<'a, '_> {
        ModalRenderCtx {
            state: self.state,
            depth: self.depth + 1,
            visited: self.visited,
            width,
            theme: self.theme,
            focused: self.focused,
            cursor: None,
            focus_range: None,
        }
    }
}

fn render_modal_node<'a>(ctx: &mut ModalRenderCtx<'a, '_>, id: &'a str) -> Vec<String> {
    if ctx.depth >= MAX_DEPTH || ctx.width == 0 || !ctx.visited.insert(id) {
        return Vec::new();
    }
    let Some(component) = ctx.state.surface.component(id) else {
        return Vec::new();
    };
    render_modal_component(ctx, component)
}

fn render_modal_component<'a>(ctx: &mut ModalRenderCtx<'a, '_>, component: &'a A2uiComponent) -> Vec<String> {
    let surface = &ctx.state.surface;
    match &component.kind {
        A2uiComponentKind::Column { children, gap } => render_modal_column(ctx, children, gap.unwrap_or(0) as usize),
        A2uiComponentKind::Row { children, gap, justify } => {
            render_modal_row(ctx, children, gap.unwrap_or(1) as usize, justify.unwrap_or_default())
        }
        A2uiComponentKind::Card { title, child, tone } => render_modal_card(ctx, title.as_ref(), child, *tone),
        A2uiComponentKind::Divider { label } => render_divider(surface, label.as_ref(), ctx.width, ctx.theme),
        A2uiComponentKind::Text { text, usage_hint, tone } => {
            render_text(surface, text, *usage_hint, *tone, ctx.width, ctx.theme)
        }
        A2uiComponentKind::Code { code, language } => {
            render_code(surface, code, language.as_deref(), ctx.width, ctx.theme)
        }
        A2uiComponentKind::Badge { label, tone } => render_badge(surface, label, *tone, ctx.width, ctx.theme),
        A2uiComponentKind::Table { headers, rows } => render_table(surface, headers, rows, ctx.width, ctx.theme),
        A2uiComponentKind::Button { label, variant, .. } => render_modal_button(ctx, &component.id, label, *variant),
        A2uiComponentKind::ChoicePicker {
            label,
            options,
            selection_path,
            multi_select,
            allow_other,
            other_path,
        } => render_modal_choice_picker(
            ctx,
            ChoicePickerRender {
                id: &component.id,
                label: label.as_ref(),
                options,
                selection_path,
                multi_select: *multi_select,
                allow_other: *allow_other,
                other_path: other_path.as_deref(),
            },
        ),
        A2uiComponentKind::TextField {
            label,
            placeholder,
            value_path,
            masked,
            ..
        } => render_modal_text_field(
            ctx,
            &component.id,
            label.as_ref(),
            placeholder.as_deref(),
            value_path,
            *masked,
        ),
        A2uiComponentKind::CheckBox { label, checked_path } => {
            render_modal_checkbox(ctx, &component.id, label, checked_path)
        }
    }
}

fn merge_child_offsets(
    ctx: &mut ModalRenderCtx<'_, '_>,
    child_cursor: Option<CursorPosition>,
    child_focus: Option<(usize, usize)>,
    base_row: usize,
    col_offset: usize,
) {
    if let Some(c) = child_cursor {
        ctx.cursor = Some(CursorPosition {
            row: base_row + c.row,
            column: col_offset + c.column,
        });
    }
    if let Some((start, end)) = child_focus {
        ctx.focus_range = Some((base_row + start, base_row + end));
    }
}

fn render_modal_column<'a>(ctx: &mut ModalRenderCtx<'a, '_>, children: &'a [String], gap: usize) -> Vec<String> {
    let mut out = Vec::new();
    let width = ctx.width;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let rendered = render_modal_node(&mut child_ctx, child_id);
        let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
        if rendered.is_empty() {
            continue;
        }
        if !out.is_empty() && gap > 0 {
            out.extend(std::iter::repeat_n(String::new(), gap));
        }
        let base_row = out.len();
        merge_child_offsets(ctx, child_cursor, child_focus, base_row, 0);
        out.extend(rendered);
    }
    out
}

fn render_modal_row<'a>(
    ctx: &mut ModalRenderCtx<'a, '_>,
    children: &'a [String],
    gap: usize,
    justify: rho_harness_core::a2ui::RowJustify,
) -> Vec<String> {
    let mut segments = Vec::new();
    let width = ctx.width;
    let mut has_focus = false;
    for child_id in children {
        let mut child_ctx = ctx.child(width);
        let lines = render_modal_node(&mut child_ctx, child_id);
        if !lines.is_empty() {
            has_focus |= child_ctx.focus_range.is_some();
            segments.push(lines.join(" "));
        }
    }
    if segments.is_empty() {
        return Vec::new();
    }
    if has_focus {
        ctx.focus_range = Some((0, 1));
    }
    vec![layout_row_segments(&segments, gap, justify, width)]
}

fn render_modal_card<'a>(
    ctx: &mut ModalRenderCtx<'a, '_>,
    title: Option<&BoundString>,
    child: &'a str,
    tone: Option<SemanticTone>,
) -> Vec<String> {
    let width = ctx.width;
    if width < 4 {
        let mut child_ctx = ctx.child(width);
        let out = render_modal_node(&mut child_ctx, child);
        let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
        merge_child_offsets(ctx, child_cursor, child_focus, 0, 0);
        return out;
    }
    let inner_width = width.saturating_sub(4).max(1);
    let mut child_ctx = ctx.child(inner_width);
    let child_lines = render_modal_node(&mut child_ctx, child);
    let (child_cursor, child_focus) = (child_ctx.cursor, child_ctx.focus_range);
    merge_child_offsets(ctx, child_cursor, child_focus, 1, 2);
    let border = card_border_style(tone, ctx.theme);
    let mut out = Vec::with_capacity(child_lines.len() + 2);
    out.push(card_top_border(&ctx.state.surface, title, width, border));
    for line in child_lines {
        let clamped = clamp_line(&line, inner_width);
        let pad = inner_width.saturating_sub(visible_width(&clamped));
        out.push(format!(
            "{border}│{border:#} {clamped}{} {border}│{border:#}",
            " ".repeat(pad)
        ));
    }
    let bottom = format!("╰{}╯", "─".repeat(width.saturating_sub(2)));
    out.push(style_line(&bottom, border));
    out
}

fn render_modal_button(
    ctx: &mut ModalRenderCtx<'_, '_>,
    id: &str,
    label: &BoundString,
    variant: ButtonVariant,
) -> Vec<String> {
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let resolved = ctx.state.surface.resolve_string(label);
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

struct ChoicePickerRender<'a> {
    id: &'a str,
    label: Option<&'a BoundString>,
    options: &'a [ChoiceOption],
    selection_path: &'a str,
    multi_select: bool,
    allow_other: bool,
    other_path: Option<&'a str>,
}

fn render_modal_choice_picker(ctx: &mut ModalRenderCtx<'_, '_>, spec: ChoicePickerRender<'_>) -> Vec<String> {
    let ChoicePickerRender {
        id,
        label,
        options,
        selection_path,
        multi_select,
        allow_other,
        other_path,
    } = spec;
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let cursor = ctx.state.choice_cursor(id);
    let mut out = Vec::new();
    if let Some(lbl) = label {
        out.extend(render_text(
            &ctx.state.surface,
            lbl,
            TextUsageHint::Body,
            None,
            ctx.width,
            ctx.theme,
        ));
    }
    let selected_single = resolve_pointer(&ctx.state.surface.data_model, selection_path).and_then(Value::as_str);
    let selected_multi = ctx.state.read_string_array(selection_path);
    let label_col_w = options
        .iter()
        .map(|opt| visible_width(&ctx.state.surface.resolve_string(&opt.label)))
        .max()
        .unwrap_or(0)
        .min((ctx.width / 2).max(1));

    for (idx, opt) in options.iter().enumerate() {
        let active_cursor = is_focused && cursor == idx;
        let checked = if multi_select {
            selected_multi.iter().any(|v| v == &opt.value)
        } else {
            selected_single == Some(opt.value.as_str())
        };
        let line = format_choice_option_line(ChoiceOptionFormat {
            surface: &ctx.state.surface,
            opt,
            active_cursor,
            checked,
            multi_select,
            label_col_w,
            max_width: ctx.width,
            theme: ctx.theme,
        });
        out.push(clamp_line(&line, ctx.width));
    }

    if allow_other {
        let active_cursor = is_focused && cursor == options.len();
        let checked = if multi_select {
            selected_multi.iter().any(|v| v == OTHER_SENTINEL)
        } else {
            selected_single == Some(OTHER_SENTINEL)
        };
        let other_text = ctx.state.read_string(other_path.unwrap_or(selection_path));
        let editing = is_focused && ctx.state.editing_other;
        let (line, col) = format_other_option_line(OtherOptionRender {
            active_cursor,
            checked,
            multi_select,
            other_text: &other_text,
            editing,
            focused: ctx.focused,
            theme: ctx.theme,
        });
        if editing {
            ctx.cursor = Some(CursorPosition {
                row: out.len(),
                column: col.min(ctx.width),
            });
        }
        out.push(clamp_line(&line, ctx.width));
    }

    if is_focused {
        ctx.focus_range = Some((0, out.len().max(1)));
    }
    out
}

struct ChoiceOptionFormat<'a> {
    surface: &'a A2uiSurface,
    opt: &'a ChoiceOption,
    active_cursor: bool,
    checked: bool,
    multi_select: bool,
    label_col_w: usize,
    max_width: usize,
    theme: &'a Theme,
}

fn truncate_description(desc: &str, max_w: usize) -> String {
    if visible_width(desc) <= max_w {
        return desc.to_string();
    }
    if max_w <= 1 {
        return "…".to_string();
    }
    format!("{}…", truncate_to_width(desc, max_w - 1))
}

fn format_choice_option_line(spec: ChoiceOptionFormat<'_>) -> String {
    let ChoiceOptionFormat {
        surface,
        opt,
        active_cursor,
        checked,
        multi_select,
        label_col_w,
        max_width,
        theme,
    } = spec;
    let prefix = if active_cursor {
        format!("{}▸{:#} ", theme.highlight, theme.highlight)
    } else {
        "  ".to_string()
    };
    let indicator = match (multi_select, checked) {
        (true, true) => format!("{}[x]{:#}", theme.tool_ok, theme.tool_ok),
        (true, false) => "[ ]".to_string(),
        (false, true) => format!("{}●{:#}", theme.highlight, theme.highlight),
        (false, false) => "○".to_string(),
    };
    let label = surface.resolve_string(&opt.label);
    let label_w = visible_width(&label);
    let styled_label = if active_cursor {
        format!("\x1b[1m{label}\x1b[0m")
    } else {
        label
    };
    let Some(desc) = &opt.description else {
        return format!("{prefix}{indicator} {styled_label}");
    };
    let pad = label_col_w.saturating_sub(label_w);
    let used_w = visible_width(&prefix) + visible_width(&indicator) + 1 + label_w + pad + 2;
    let desc_max = max_width.saturating_sub(used_w);
    if desc_max == 0 {
        return format!("{prefix}{indicator} {styled_label}");
    }
    let d = surface.resolve_string(desc);
    let truncated_desc = truncate_description(&d, desc_max);
    format!(
        "{prefix}{indicator} {styled_label}{}  {}{truncated_desc}{:#}",
        " ".repeat(pad),
        theme.dimmed,
        theme.dimmed
    )
}

struct OtherOptionRender<'a> {
    active_cursor: bool,
    checked: bool,
    multi_select: bool,
    other_text: &'a str,
    editing: bool,
    focused: bool,
    theme: &'a Theme,
}

fn format_other_option_line(spec: OtherOptionRender<'_>) -> (String, usize) {
    let OtherOptionRender {
        active_cursor,
        checked,
        multi_select,
        other_text,
        editing,
        focused,
        theme,
    } = spec;
    let prefix = if active_cursor {
        format!("{}▸{:#} ", theme.highlight, theme.highlight)
    } else {
        "  ".to_string()
    };
    let indicator = match (multi_select, checked) {
        (true, true) => format!("{}[x]{:#}", theme.tool_ok, theme.tool_ok),
        (true, false) => "[ ]".to_string(),
        (false, true) => format!("{}●{:#}", theme.highlight, theme.highlight),
        (false, false) => "○".to_string(),
    };
    if !checked && !editing && other_text.is_empty() {
        let label = if active_cursor {
            "\x1b[1mOther…\x1b[0m"
        } else {
            "Other…"
        };
        return (format!("{prefix}{indicator} {label}"), 0);
    }
    let base = format!("{prefix}{indicator} Other: [");
    let col = visible_width(&base) + visible_width(other_text);
    let cursor_block = if editing && focused && theme.cursor_mode == CursorMode::Software {
        "\x1b[7m \x1b[27m"
    } else {
        ""
    };
    (format!("{base}{other_text}{cursor_block}]"), col)
}

fn render_modal_text_field(
    ctx: &mut ModalRenderCtx<'_, '_>,
    id: &str,
    label: Option<&BoundString>,
    placeholder: Option<&str>,
    value_path: &str,
    masked: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(lbl) = label {
        out.extend(render_text(
            &ctx.state.surface,
            lbl,
            TextUsageHint::Body,
            None,
            ctx.width,
            ctx.theme,
        ));
    }
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let val = ctx.state.read_string(value_path);
    let display = if val.is_empty() && !is_focused {
        let ph = placeholder.unwrap_or("");
        format!("{}{ph}{:#}", ctx.theme.dimmed, ctx.theme.dimmed)
    } else if masked {
        "*".repeat(val.chars().count())
    } else {
        val
    };
    let prefix = if is_focused {
        format!("{}▸{:#} ", ctx.theme.highlight, ctx.theme.highlight)
    } else {
        "  ".to_string()
    };
    let head = format!("{prefix}[");
    let col = (visible_width(&head) + visible_width(&display)).min(ctx.width);
    let cursor_block = if is_focused && ctx.focused && ctx.theme.cursor_mode == CursorMode::Software {
        "\x1b[7m \x1b[27m"
    } else {
        ""
    };
    let input_row = out.len();
    out.push(clamp_line(&format!("{head}{display}{cursor_block}]"), ctx.width));
    if is_focused {
        ctx.cursor = Some(CursorPosition {
            row: input_row,
            column: col,
        });
        ctx.focus_range = Some((0, out.len().max(1)));
    }
    out
}

fn render_modal_checkbox(
    ctx: &mut ModalRenderCtx<'_, '_>,
    id: &str,
    label: &BoundString,
    checked_path: &str,
) -> Vec<String> {
    let is_focused = ctx.state.focused_component_id() == Some(id);
    let checked = resolve_pointer(&ctx.state.surface.data_model, checked_path)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let prefix = if is_focused {
        format!("{}▸{:#} ", ctx.theme.highlight, ctx.theme.highlight)
    } else {
        "  ".to_string()
    };
    let mark = if checked { "[x]" } else { "[ ]" };
    let text = ctx.state.surface.resolve_string(label);
    let line = format!("{prefix}{mark} {text}");
    let styled = if checked {
        style_line(&line, ctx.theme.tool_ok)
    } else if is_focused {
        format!("\x1b[1m{line}\x1b[0m")
    } else {
        line
    };
    if is_focused {
        ctx.focus_range = Some((0, 1));
    }
    vec![clamp_line(&styled, ctx.width)]
}

fn window_around_focus(
    lines: Vec<String>,
    focus_range: Option<(usize, usize)>,
    max_lines: usize,
    width: usize,
    theme: &Theme,
) -> (Vec<String>, usize) {
    if lines.len() <= max_lines {
        return (lines, 0);
    }
    let keep = max_lines.saturating_sub(1).max(1);
    let focus_mid = focus_range.map_or(0, |(s, e)| (s + e) / 2);
    let start = focus_mid.saturating_sub(keep / 2).min(lines.len().saturating_sub(keep));
    let mut visible: Vec<String> = lines.iter().skip(start).take(keep).cloned().collect();
    let hidden = lines.len().saturating_sub(visible.len());
    if hidden > 0 && visible.len() < max_lines {
        let notice = format!("{}... (+{hidden} lines){:#}", theme.dimmed, theme.dimmed);
        visible.push(clamp_line(&truncate_to_width(&notice, width), width));
    }
    (visible, start)
}

fn adjust_cursor(
    cursor: Option<CursorPosition>,
    offset: usize,
    visible_len: usize,
    focused: bool,
) -> (CursorPosition, bool) {
    if let Some(c) = cursor
        && c.row >= offset
        && c.row - offset < visible_len
    {
        return (
            CursorPosition {
                row: c.row - offset,
                column: c.column,
            },
            focused,
        );
    }
    (CursorPosition { row: 0, column: 0 }, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::strip_ansi;
    use rho_harness_core::a2ui::{
        A2uiMessage, CreateSurface, RowJustify, SurfacePlacement, UpdateComponents, UpdateDataModel,
    };
    use serde_json::json;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    fn single_select_request(allow_other: bool) -> A2uiModalRequest {
        A2uiModalRequest {
            surface_id: "rho:ask".to_string(),
            title: Some("Database".to_string()),
            messages: vec![
                A2uiMessage::CreateSurface(CreateSurface {
                    surface_id: "rho:ask".to_string(),
                    catalog_id: None,
                    placement: SurfacePlacement::Modal,
                    title: Some("Database".to_string()),
                    priority: 100,
                    root_id: "root".to_string(),
                }),
                A2uiMessage::UpdateComponents(UpdateComponents {
                    surface_id: "rho:ask".to_string(),
                    components: vec![
                        A2uiComponent::new(
                            "q1",
                            A2uiComponentKind::ChoicePicker {
                                label: Some(BoundString::literal("Pick a database")),
                                options: vec![
                                    ChoiceOption {
                                        value: "0".to_string(),
                                        label: BoundString::literal("PostgreSQL"),
                                        description: Some(BoundString::literal("Relational")),
                                    },
                                    ChoiceOption {
                                        value: "1".to_string(),
                                        label: BoundString::literal("SQLite"),
                                        description: None,
                                    },
                                ],
                                selection_path: "/answers/q1".to_string(),
                                multi_select: false,
                                allow_other,
                                other_path: allow_other.then(|| "/answers/q1_other".to_string()),
                            },
                        ),
                        A2uiComponent::new(
                            "cancel_btn",
                            A2uiComponentKind::Button {
                                label: BoundString::literal("Cancel"),
                                action: ActionBinding {
                                    name: "cancel".to_string(),
                                    context: BTreeMap::new(),
                                },
                                variant: ButtonVariant::Secondary,
                            },
                        ),
                        A2uiComponent::new(
                            "submit_btn",
                            A2uiComponentKind::Button {
                                label: BoundString::literal("Submit"),
                                action: ActionBinding {
                                    name: "submit".to_string(),
                                    context: BTreeMap::new(),
                                },
                                variant: ButtonVariant::Primary,
                            },
                        ),
                        A2uiComponent::new(
                            "actions",
                            A2uiComponentKind::Row {
                                children: vec!["cancel_btn".to_string(), "submit_btn".to_string()],
                                gap: Some(1),
                                justify: Some(RowJustify::End),
                            },
                        ),
                        A2uiComponent::new(
                            "root",
                            A2uiComponentKind::Column {
                                children: vec!["q1".to_string(), "actions".to_string()],
                                gap: Some(1),
                            },
                        ),
                    ],
                }),
                A2uiMessage::UpdateDataModel(UpdateDataModel {
                    surface_id: "rho:ask".to_string(),
                    path: None,
                    value: json!({ "answers": { "q1": null } }),
                }),
            ],
            timeout_ms: None,
        }
    }

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
                A2uiMessage::CreateSurface(CreateSurface {
                    surface_id: "form".to_string(),
                    catalog_id: None,
                    placement: SurfacePlacement::Modal,
                    title: None,
                    priority: 100,
                    root_id: "root".to_string(),
                }),
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
                A2uiMessage::CreateSurface(CreateSurface {
                    surface_id: "secret".to_string(),
                    catalog_id: None,
                    placement: SurfacePlacement::Modal,
                    title: None,
                    priority: 100,
                    root_id: "token".to_string(),
                }),
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
    fn other_option_renders_ellipsis_when_idle_and_box_when_active() {
        let mut state = A2uiModalState::from_request(single_select_request(true)).unwrap();
        let (lines, _, _) = state.render(60, 10, &Theme::default(), true);
        let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
        let other_line = plain.iter().find(|l| l.contains("Other")).expect("other line");
        assert!(other_line.contains("○ Other…"), "got: {other_line}");
        assert!(!other_line.contains("Other: []"));

        // Activate Other by digit jump '3'
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
                A2uiMessage::CreateSurface(CreateSurface {
                    surface_id: "align".to_string(),
                    catalog_id: None,
                    placement: SurfacePlacement::Modal,
                    title: None,
                    priority: 100,
                    root_id: "q1".to_string(),
                }),
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
