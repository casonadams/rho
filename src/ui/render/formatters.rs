//! Edit-diff, write-preview, and thinking-block formatters.
//!
//! These are `pub(crate)` because they are only consumed by `renderer.rs`,
//! but they remain exposed as module-private items so future tools can reuse them.

use crate::ui::block::BlockFormat;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::summary::{approval_heading, bash_approval_details};
use rho_harness_core::presentation::{BashApproval, RiskTier, SessionStatus};

pub(super) fn format_bash_approval_card(request: &BashApproval, theme: &Theme, width: usize) -> String {
    let high_risk = request.tier == RiskTier::HighRisk;
    let title = anstyle::Style::new().bold().fg_color(Some(if high_risk {
        anstyle::AnsiColor::Red.into()
    } else {
        anstyle::AnsiColor::Yellow.into()
    }));
    let background = if high_risk {
        theme.tool_error_bg
    } else {
        theme.tool_success_bg
    };
    let accent = theme.highlight;
    let error = theme.tool_err;
    let mut content = format!("{title}{}{title:#}", approval_heading(request.tier));
    for (index, line) in bash_approval_details(request).iter().enumerate() {
        content.push('\n');
        if line.is_empty() {
            continue;
        }
        if index == 0 {
            content.push_str(&format!("{accent}{line}{accent:#}"));
        } else {
            content.push_str(&format!("{error}! {line}{error:#}"));
        }
    }
    BlockFormat::new(background, width)
        .with_vertical_padding()
        .render_styled(&content)
}

pub(crate) fn format_edit_diff(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let edits = args.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }
    let d = theme.dimmed;
    let mut out = format!("{d}```diff{d:#}\n");
    for (idx, edit) in edits.iter().enumerate() {
        let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(&super::diff::format_entry_diff(super::diff::EntryDiffInput {
            idx,
            old_text,
            new_text,
            theme,
        }));
    }
    out.push_str(&format!("{d}```{d:#}\n"));
    Some(out)
}

pub(crate) fn format_write_preview(args: &serde_json::Value, theme: &Theme, expanded: bool) -> Option<String> {
    let content = args.get("content")?.as_str()?;
    if content.trim().is_empty() {
        return None;
    }
    let d = theme.dimmed;
    let green = theme.tool_ok;
    let mut out = format!("{d}```diff{d:#}\n");
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let max = if expanded { total } else { 8.min(total) };
    for line in &lines[..max] {
        let no_tabs = line.replace('\t', "   ");
        out.push_str(&format!("{green}+ {no_tabs}{green:#}\n"));
    }
    if !expanded && total > 8 {
        out.push_str(&format!(
            "{d}... ({} more lines, {total} total, Ctrl+O to expand){d:#}\n",
            total - 8
        ));
    }
    out.push_str(&format!("{d}```{d:#}\n"));
    Some(out)
}

pub fn format_session_status(session: &SessionStatus) -> String {
    let mut parts = vec![session.model.clone(), session.context.to_string()];
    if let Some(usage) = session.quota.as_deref() {
        parts.push(usage.to_string());
    }
    parts.join(" | ")
}

pub(crate) fn format_thinking_block(thinking_text: &str, theme: &Theme) -> String {
    let d = theme.dimmed;
    let mut out = String::from("\n");
    for line in thinking_text.trim().lines() {
        out.push_str(&format!("{d}{line}{d:#}\n"));
    }
    out.push('\n');
    out
}
