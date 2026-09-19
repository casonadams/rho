pub mod horizontal;
pub mod input;
pub mod types;
pub mod vertical;

pub use input::{in_input_modal_desired_lines, modal_body_max_scroll, render_in_input_modal};
pub use types::InInputModalInput;
pub use vertical::modal_hint;

#[cfg(test)]
pub mod in_input {
    pub use super::input::*;
}

#[cfg(test)]
pub mod options {
    pub use super::types::*;
    pub use super::vertical::*;
}
