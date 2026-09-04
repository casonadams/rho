use super::formatters::{format_bash_approval_card, format_session_status};
use super::renderer::TerminalRenderer;
use crate::ui::block::{BlockFormat, terminal_width};
use rho_harness_core::presentation::summary::to_relative_path;
use rho_harness_core::presentation::{BashApproval, BlockDisplay, SessionStatus, WelcomeDisplay};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CacheMissNotice {
    pub missed_tokens: u64,
    pub cost: Option<f64>,
    pub idle_minutes: Option<u64>,
}

impl TerminalRenderer {
    pub fn print_welcome(&self, display: &WelcomeDisplay) {
        let location = std::env::current_dir()
            .ok()
            .map(|path| to_relative_path(&path.display().to_string()))
            .unwrap_or_else(|| ".".to_string());
        let item = crate::ui::interactive::WelcomeItem {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: display.model.to_string(),
            provider: display.provider.to_string(),
            auto_approve: display.auto_approve,
            resumed: display.resumed,
            location,
            tools: display.tools.clone(),
            skills: display.skills.clone(),
            plugins: display.plugins.clone(),
        };
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Welcome(item));
        } else {
            let text = crate::ui::interactive::format_welcome_content(&item, &self.theme);
            self.write_output(&text);
        }
    }

    pub fn print_session_status(&self, display: &SessionStatus) {
        let dim = self.theme.dimmed;
        let status = format_session_status(display);
        let text = format!("{dim}{status}{dim:#}\n");
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(text));
        } else {
            self.write_output(&text);
        }
    }

    pub fn print_block(&self, display: &BlockDisplay) {
        let bg = match display.style.as_str() {
            "error" => self.theme.tool_error_bg,
            "warning" => anstyle::Style::new()
                .bg_color(Some(anstyle::AnsiColor::Yellow.into()))
                .fg_color(Some(anstyle::AnsiColor::Black.into())),
            "success" => self.theme.tool_success_bg,
            _ => self.theme.user_message_bg,
        };
        let formatted_title = if display.title.is_empty() {
            String::new()
        } else {
            let bold = anstyle::Style::new().bold();
            format!("{bold}{}{bold:#}\n\n", display.title)
        };
        let full_text = format!("{formatted_title}{}", display.content);
        let rendered = BlockFormat::new(bg, terminal_width())
            .with_vertical_padding()
            .render_styled(&full_text);
        let block_output = format!("\n{rendered}\n");
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(block_output));
        } else {
            self.write_output(&block_output);
        }
    }

    pub fn print_notice(&self, text: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(text.to_string()));
        } else {
            self.write_output(text);
        }
    }

    pub fn print_status(&self, message: &str) {
        let dim = self.theme.dimmed;
        let text = format!("{dim}{message}{dim:#}\n");
        self.print_notice(&text);
    }

    pub fn print_compaction_cost_notice(&self, tokens: u64, cost: Option<f64>) {
        let warning = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let cost_str = cost
            .filter(|c| *c >= 0.01)
            .map(|c| format!(" (~${c:.2})"))
            .unwrap_or_default();
        let tokens_str = crate::ui::interactive::footer::format_tokens(tokens);
        let text = format!("{warning}Compaction: {tokens_str} tokens billed{cost_str}{warning:#}\n");
        self.print_notice(&text);
    }

    pub fn print_cache_miss_notice(&self, notice: CacheMissNotice) {
        let warning = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let cost_str = notice
            .cost
            .filter(|c| *c >= 0.01)
            .map(|c| format!(" (~${c:.2})"))
            .unwrap_or_default();
        let tokens_str = crate::ui::interactive::footer::format_tokens(notice.missed_tokens);
        let reason = if let Some(mins) = notice.idle_minutes {
            format!("after {mins}m idle")
        } else {
            "after model switch".to_string()
        };
        let text = format!("{warning}Cache miss {reason}: {tokens_str} tokens re-billed{cost_str}{warning:#}\n");
        self.print_notice(&text);
    }

    pub fn print_user_block(&self, input: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::UserMessage(input.to_string()));
        } else {
            let user = self.theme.prompt;
            self.write_output(&format!("{user}>{user:#} {input}\n\n"));
        }
    }

    pub fn print_bash_approval_request(&self, request: &BashApproval) {
        let card = format_bash_approval_card(request, &self.theme, terminal_width());
        self.write_output(&format!("\n{card}\n"));
    }
}
