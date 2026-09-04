//! Tool execution dispatch and output rendering for TerminalRenderer.

use super::TerminalRenderer;
use crate::ui::render::card::render_headless_tool_card;
use crate::ui::render::formatters::{format_edit_diff, format_write_preview};
use rho_harness_core::presentation::summary::format_tool_args_summary;
use rho_harness_core::presentation::{ToolLine, ToolOutcome};

impl TerminalRenderer {
    pub fn start_tool_run(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let preview = if name == "edit" {
            format_edit_diff(args, &self.theme)
        } else if name == "write" {
            format_write_preview(args, &self.theme, false)
        } else {
            None
        };
        if let Some(ui) = &self.ui {
            let has_running_widget = preview.is_some() || name == "bash" || name == "write";
            if has_running_widget {
                let _ = ui.tool_start(crate::ui::interactive::ToolStartRequest {
                    name: name.to_string(),
                    args_summary: summary,
                    preview,
                });
            } else {
                let _ = ui.set_running_tool(Some(name.to_string()));
            }
        } else {
            self.print_tool_start(name, args);
        }
    }

    pub fn tool_chunk(&self, chunk: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.tool_chunk(chunk.to_string());
        }
    }

    pub fn finish_tool_line(&self, line: ToolLine) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: line.name,
                    arguments: line.arguments,
                    is_error: line.is_error,
                    output: line.output,
                    output_summary: line.output_summary,
                    duration_ms: line.duration_ms,
                },
            ));
            return;
        }
        let card = render_headless_tool_card(&line, &self.theme);
        self.write_output(&card);
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        self.write_output(&format!("\n{header}{name}{header:#} {dim}{summary}{dim:#}\n"));
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome) {
        if outcome.is_error {
            let err = self.theme.tool_err;
            self.write_output(&format!(
                "{err}{} failed:{err:#} {}\n",
                outcome.name, outcome.output_summary
            ));
        } else {
            let ok = self.theme.tool_ok;
            self.write_output(&format!("{ok}{}{ok:#}\n", outcome.name));
        }
    }
}
