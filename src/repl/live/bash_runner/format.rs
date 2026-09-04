use rho_engine::tools::bash::OutputSnapshot;
use rho_harness_core::presentation::ToolLine;

pub struct UserBashResult {
    pub output: String,
    pub is_cancelled: bool,
    pub is_error: bool,
}

pub(super) struct BashOutcome {
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub args_val: serde_json::Value,
}

pub(super) fn finish_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome) -> (ToolLine, UserBashResult) {
    if let Some(code) = outcome.exit_code {
        let is_error = code != 0;
        let output = format_bash_output(snapshot, code);
        let summary = if is_error {
            format!("exit {code}")
        } else {
            "completed".to_string()
        };
        (
            ToolLine {
                name: "bash".to_string(),
                arguments: outcome.args_val,
                is_error,
                output: output.clone(),
                output_summary: summary,
                duration_ms: Some(outcome.duration_ms),
            },
            UserBashResult {
                output,
                is_cancelled: false,
                is_error,
            },
        )
    } else {
        let output = format_cancel_output(snapshot);
        (
            ToolLine {
                name: "bash".to_string(),
                arguments: outcome.args_val,
                is_error: true,
                output: output.clone(),
                output_summary: "(cancelled)".to_string(),
                duration_ms: Some(outcome.duration_ms),
            },
            UserBashResult {
                output,
                is_cancelled: true,
                is_error: true,
            },
        )
    }
}

fn format_bash_output(snapshot: &OutputSnapshot, exit_code: i32) -> String {
    let output_trimmed = snapshot.formatted_text.trim();
    if exit_code != 0 {
        let status_msg = format!("Command exited with code {exit_code}");
        if output_trimmed.is_empty() {
            status_msg
        } else {
            format!("{output_trimmed}\n\n{status_msg}")
        }
    } else if output_trimmed.is_empty() {
        "[Command completed with exit code 0 (no output)]".to_string()
    } else {
        snapshot.formatted_text.clone()
    }
}

fn format_cancel_output(snapshot: &OutputSnapshot) -> String {
    let output_trimmed = snapshot.formatted_text.trim();
    if output_trimmed.is_empty() {
        "(cancelled)".to_string()
    } else {
        format!("{output_trimmed}\n(cancelled)")
    }
}
