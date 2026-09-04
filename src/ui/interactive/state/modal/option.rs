use crate::ui::interactive::{InteractionInput, InteractionOption};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalOption {
    pub label: String,
    pub description: Option<String>,
    /// Inline input shown at the bottom of the same modal when this option
    /// is chosen; the submitted text travels back with the selection.
    pub input: Option<InteractionInput>,
}

impl ModalOption {
    pub fn new(label: impl Into<String>, description: Option<impl Into<String>>) -> Self {
        Self {
            label: label.into(),
            description: description.map(Into::into),
            input: None,
        }
    }
}

impl From<String> for ModalOption {
    fn from(label: String) -> Self {
        Self {
            label,
            description: None,
            input: None,
        }
    }
}

impl From<&str> for ModalOption {
    fn from(label: &str) -> Self {
        Self {
            label: label.to_string(),
            description: None,
            input: None,
        }
    }
}

impl From<InteractionOption> for ModalOption {
    fn from(opt: InteractionOption) -> Self {
        Self {
            label: opt.label,
            description: opt.description,
            input: opt.input,
        }
    }
}
