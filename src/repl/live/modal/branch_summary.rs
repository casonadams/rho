use crossterm::event::KeyEvent;

use super::ModalKeyResult;
use crate::error::Result;
use crate::ui::interactive::{ModalKind, ModalOption, ModalState, TerminalBackend, TerminalController};

pub fn open_branch_summary_modal<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    leaf_id: String,
    old_leaf: String,
) {
    let options = vec![
        ModalOption::new(format!("{:14}", "Yes"), Some("Summarize discoveries before switching")).with_value("yes"),
        ModalOption::new(format!("{:14}", "No"), Some("Switch without a summary")).with_value("no"),
    ];
    let modal = ModalState::new("Summarize Abandoned Branch", "", options)
        .with_kind(ModalKind::BranchSummary { leaf_id, old_leaf });
    controller.state_mut().push_modal(modal);
}

pub fn handle_branch_summary_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    leaf_id: String,
    old_leaf: String,
) -> Result<ModalKeyResult> {
    super::dispatch_simple_selector(controller, key, |opt| {
        let summarize = opt.value.as_deref()? == "yes";
        Some(ModalKeyResult::BranchSummaryChosen {
            leaf_id,
            old_leaf,
            summarize,
        })
    })
}
