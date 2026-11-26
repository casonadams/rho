use rho_engine::tools::bash::runner::{cancelled_text, exit_text};
use rho_engine::tools::bash::{OutputAccumulator, OutputSnapshot};
use rho_harness_core::presentation::ToolLine;
use tokio::sync::mpsc::Receiver;

use crate::ui::TerminalRenderer;

pub struct UserBashResult {
    pub output: String,
    pub is_cancelled: bool,
    pub is_error: bool,
}

pub struct BashOutcome {
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub args_val: serde_json::Value,
}

pub fn finalize_run(
    chunk_rx: &mut Receiver<String>,
    accumulator: &mut OutputAccumulator,
    renderer: &TerminalRenderer,
) -> OutputSnapshot {
    while let Ok(chunk) = chunk_rx.try_recv() {
        accumulator.append(chunk.as_bytes());
        renderer.tool_chunk(&chunk);
    }
    accumulator.finish();
    accumulator.snapshot()
}

pub fn completed_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome, code: i32) -> (ToolLine, UserBashResult) {
    let is_error = code != 0;
    let output = exit_text(snapshot, code);
    let summary = if is_error {
        format!("exit {code}")
    } else {
        "completed".to_string()
    };
    let mut line = ToolLine::new("bash", outcome.args_val)
        .with_output(output.clone())
        .with_output_summary(summary)
        .with_duration_ms(outcome.duration_ms);
    if is_error {
        line = line.failed();
    }
    (
        line,
        UserBashResult {
            output,
            is_cancelled: false,
            is_error,
        },
    )
}

pub fn cancelled_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome) -> (ToolLine, UserBashResult) {
    let output = cancelled_text(snapshot);
    (
        ToolLine::new("bash", outcome.args_val)
            .with_output(output.clone())
            .with_output_summary("(cancelled)")
            .with_duration_ms(outcome.duration_ms)
            .failed(),
        UserBashResult {
            output,
            is_cancelled: true,
            is_error: true,
        },
    )
}

pub fn finish_bash_result(snapshot: &OutputSnapshot, outcome: BashOutcome) -> (ToolLine, UserBashResult) {
    match outcome.exit_code {
        Some(code) => completed_bash_result(snapshot, outcome, code),
        None => cancelled_bash_result(snapshot, outcome),
    }
}
