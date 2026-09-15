use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    AllowOnce,
    AllowAlways,
    Deny { reason: Option<String> },
    Edit { mutated_command: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPromptState {
    pub is_active: bool,
    pub tool_name: String,
    pub command_display: String,
    pub arguments: serde_json::Value,
    pub selected_index: usize,
    pub custom_deny_reason: String,
    pub edited_command: String,
    pub is_editing: bool,
}

impl Default for PermissionPromptState {
    fn default() -> Self {
        Self {
            is_active: false,
            tool_name: String::new(),
            command_display: String::new(),
            arguments: serde_json::Value::Null,
            selected_index: 0,
            custom_deny_reason: String::new(),
            edited_command: String::new(),
            is_editing: false,
        }
    }
}

pub const PERMISSION_ACTIONS: &[(&str, &str)] = &[
    ("Allow once", "Execute this command once"),
    ("Allow always", "Add matching rule to permissions file"),
    ("Deny", "Reject tool execution"),
    ("Edit", "Modify command before execution"),
];

impl PermissionPromptState {
    pub fn new(tool_name: impl Into<String>, command_display: impl Into<String>, arguments: serde_json::Value) -> Self {
        let cmd = command_display.into();
        Self {
            is_active: true,
            tool_name: tool_name.into(),
            command_display: cmd.clone(),
            arguments,
            selected_index: 0,
            custom_deny_reason: String::new(),
            edited_command: cmd,
            is_editing: false,
        }
    }

    pub fn select_next(&mut self) {
        if !self.is_editing {
            self.selected_index = (self.selected_index + 1) % PERMISSION_ACTIONS.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.is_editing {
            if self.selected_index == 0 {
                self.selected_index = PERMISSION_ACTIONS.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    pub fn start_editing(&mut self) {
        self.is_editing = true;
        self.selected_index = 3;
    }

    pub fn cancel_editing(&mut self) {
        self.is_editing = false;
        self.edited_command = self.command_display.clone();
    }

    pub fn set_edited_command(&mut self, cmd: impl Into<String>) {
        self.edited_command = cmd.into();
    }

    pub fn set_deny_reason(&mut self, reason: impl Into<String>) {
        self.custom_deny_reason = reason.into();
    }

    pub fn resolve(&mut self) -> Option<PermissionAction> {
        if !self.is_active {
            return None;
        }
        let action = match self.selected_index {
            0 => PermissionAction::AllowOnce,
            1 => PermissionAction::AllowAlways,
            2 => {
                let reason = if self.custom_deny_reason.trim().is_empty() {
                    None
                } else {
                    Some(self.custom_deny_reason.trim().to_string())
                };
                PermissionAction::Deny { reason }
            }
            3 => PermissionAction::Edit {
                mutated_command: self.edited_command.clone(),
            },
            _ => PermissionAction::Deny { reason: None },
        };
        self.is_active = false;
        Some(action)
    }
}
