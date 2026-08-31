use super::editor::EditorState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalOption {
    pub label: String,
    pub description: Option<String>,
}

impl ModalOption {
    pub fn new(label: impl Into<String>, description: Option<impl Into<String>>) -> Self {
        Self {
            label: label.into(),
            description: description.map(Into::into),
        }
    }
}

impl From<String> for ModalOption {
    fn from(label: String) -> Self {
        Self {
            label,
            description: None,
        }
    }
}

impl From<&str> for ModalOption {
    fn from(label: &str) -> Self {
        Self {
            label: label.to_string(),
            description: None,
        }
    }
}

impl From<crate::ui::interactive::InteractionOption> for ModalOption {
    fn from(opt: crate::ui::interactive::InteractionOption) -> Self {
        Self {
            label: opt.label,
            description: opt.description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModalMode {
    #[default]
    Select,
    Input {
        prompt_label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalState {
    pub title: String,
    pub body: String,
    pub options: Vec<ModalOption>,
    pub selected: usize,
    pub mode: ModalMode,
    pub input: EditorState,
    pub allow_custom: bool,
}

impl ModalState {
    pub fn new(title: impl Into<String>, body: impl Into<String>, options: Vec<ModalOption>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            options,
            selected: 0,
            mode: ModalMode::Select,
            input: EditorState::default(),
            allow_custom: false,
        }
    }

    pub fn with_custom(mut self, allow_custom: bool) -> Self {
        self.allow_custom = allow_custom;
        self
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + 1).min(self.options.len() - 1);
        }
    }

    pub fn selected_option(&self) -> Option<&ModalOption> {
        self.options.get(self.selected)
    }

    pub fn enter_input_mode(&mut self, prompt_label: impl Into<String>) {
        self.mode = ModalMode::Input {
            prompt_label: prompt_label.into(),
        };
        self.input.set_text("");
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = ModalMode::Select;
        self.input.set_text("");
    }
}
