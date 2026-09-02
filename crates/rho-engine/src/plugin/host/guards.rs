use super::types::{HostUiConfirmResult, HostUiInputResult, HostUiSelectResult};
use rho_harness_core::presentation::presenter::Presenter;

pub struct HeadlessGuard;

impl HeadlessGuard {
    pub fn is_headless(presenter: &dyn Presenter) -> bool {
        !presenter.has_interactive_ui()
    }

    pub fn fail_closed_confirm() -> HostUiConfirmResult {
        HostUiConfirmResult { confirmed: false }
    }

    pub fn fail_closed_select() -> HostUiSelectResult {
        HostUiSelectResult {
            selected: None,
            custom: None,
            cancelled: true,
        }
    }

    pub fn fail_closed_input() -> HostUiInputResult {
        HostUiInputResult {
            value: None,
            cancelled: true,
        }
    }
}
