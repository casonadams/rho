use crate::ui::interactive::ModalState;

pub struct OptionFormat<'a> {
    pub is_selected: bool,
    pub is_selector: bool,
    pub theme: &'a crate::ui::theme::Theme,
}

pub struct ModalOptionsLayout<'a> {
    pub inner_width: usize,
    pub max_visible: usize,
    pub theme: &'a crate::ui::theme::Theme,
}

pub struct InInputModalInput<'a> {
    pub modal: &'a ModalState,
    pub draft_text: &'a str,
    pub bounds: (usize, usize),
    pub theme: &'a crate::ui::theme::Theme,
    pub focused: bool,
}
