//! Interactive and progress spinner activities for TerminalRenderer.

use super::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi};
use rho_harness_core::presentation::summary::format_tool_args_summary;

pub enum RenderActivity {
    Interactive(InteractiveUi),
    Headless,
}

impl RenderActivity {
    pub fn finish_and_clear(self) {
        if let Self::Interactive(ui) = self {
            let _ = ui.set_activity(Activity::Idle);
        }
    }
}

impl TerminalRenderer {
    pub fn start_spinner(&self, message: &str) -> RenderActivity {
        if let Some(ui) = &self.ui {
            let activity = if message.starts_with("thinking") {
                Activity::Thinking
            } else if message.starts_with("compacting") {
                Activity::Compacting
            } else {
                Activity::Working
            };
            let _ = ui.set_activity(activity);
            return RenderActivity::Interactive(ui.clone());
        }
        RenderActivity::Headless
    }

    pub fn start_tool_spinner(&self, name: &str, args: &serde_json::Value) -> RenderActivity {
        let summary = format_tool_args_summary(name, args);
        let msg = format!("{name} {summary}");
        self.start_spinner(&msg)
    }
}
