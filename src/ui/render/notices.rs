use super::formatters::format_session_status;
use super::renderer::TerminalRenderer;
use crate::ui::best_effort;
use crate::ui::block::{BlockFormat, terminal_width};
use crate::ui::theme::Theme;
use rho_harness_core::presentation::summary::to_relative_path;
use rho_harness_core::presentation::{BlockDisplay, SessionStatus, WelcomeDisplay};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warn,
    Error,
}

pub fn format_notice(theme: &Theme, kind: NoticeKind, text: &str) -> String {
    let (label, style) = match kind {
        NoticeKind::Info => ("info", theme.highlight),
        NoticeKind::Warn => ("warning", theme.warning),
        NoticeKind::Error => ("error", theme.tool_err),
    };
    let style = style.bold();
    let body = text.trim().replace('\n', "\n ");
    format!("\n {style}{label}{style:#} {body}\n")
}

fn resolve_block_bg(theme: &Theme, style: &str) -> anstyle::Style {
    match style {
        "warning" => {
            if let Some(fg) = theme.warning.get_fg_color() {
                anstyle::Style::new()
                    .bg_color(Some(fg))
                    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Black)))
            } else {
                anstyle::Style::new()
                    .bg_color(Some(anstyle::AnsiColor::Yellow.into()))
                    .fg_color(Some(anstyle::AnsiColor::Black.into()))
            }
        }
        _ => theme.block_fill,
    }
}

fn format_block_title(title: &str) -> String {
    if title.is_empty() {
        String::new()
    } else {
        let bold = anstyle::Style::new().bold();
        format!("{bold}{title}{bold:#}\n\n")
    }
}

fn resolve_block_format(theme: &Theme, style: &str) -> BlockFormat {
    let border = if style == "warning" {
        theme.warning
    } else {
        theme.agent_border
    };
    match theme.block_style {
        crate::ui::theme::BlockStyle::Solid => BlockFormat::new(resolve_block_bg(theme, style), terminal_width()),
        crate::ui::theme::BlockStyle::Border => BlockFormat::border(border, terminal_width()),
        crate::ui::theme::BlockStyle::Divider => {
            BlockFormat::divider(resolve_block_bg(theme, style), border, terminal_width())
        }
    }
}

pub fn render_block_content(theme: &Theme, display: &BlockDisplay) -> String {
    let formatted_title = format_block_title(&display.title);
    let full_text = format!("{formatted_title}{}", display.content);
    let block_fmt = resolve_block_format(theme, &display.style);
    let rendered = block_fmt.with_vertical_padding().render_styled(&full_text);
    format!("\n{rendered}\n")
}

impl TerminalRenderer {
    pub fn print_welcome(&self, display: &WelcomeDisplay) {
        let location = std::env::current_dir()
            .ok()
            .map(|path| to_relative_path(&path.display().to_string()))
            .unwrap_or_else(|| ".".to_string());
        let agents = display.agents.iter().map(|path| to_relative_path(path)).collect();
        let item = crate::ui::interactive::WelcomeItem {
            version: rho_harness_core::build_info::welcome_version().to_string(),
            model: display.model.to_string(),
            provider: display.provider.to_string(),
            resumed: display.resumed,
            location,
            agents,
            tools: display.tools.clone(),
            skills: display.skills.clone(),
            mcp: display.mcp.clone(),
        };
        if let Some(ui) = &self.ui {
            best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::Welcome(item)));
        } else {
            let text = crate::ui::interactive::format_welcome_content(&item, terminal_width(), &self.theme);
            self.write_output(&text);
        }
    }

    fn emit_notice_output(&self, text: &str) {
        if let Some(ui) = &self.ui {
            best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(text.to_string())));
        } else {
            self.write_output(text);
        }
    }

    pub fn print_session_status(&self, display: &SessionStatus) {
        let dim = self.theme.dimmed;
        let status = format_session_status(display);
        let text = format!("{dim}{status}{dim:#}\n");
        self.emit_notice_output(&text);
    }

    pub fn print_block(&self, display: &BlockDisplay) {
        let block_output = render_block_content(&self.theme, display);
        self.emit_notice_output(&block_output);
    }

    pub fn notice(&self, kind: NoticeKind, text: &str) {
        self.emit_notice_output(&format_notice(&self.theme, kind, text));
    }

    pub fn print_notice(&self, text: &str) {
        self.emit_notice_output(text);
    }

    pub fn print_status(&self, message: &str) {
        if let Some(ui) = &self.ui {
            best_effort(ui.set_system_message(Some(message.to_string())));
        } else {
            let dim = self.theme.dimmed;
            let text = format!("{dim}{message}{dim:#}\n");
            self.write_output(&text);
        }
    }

    pub fn print_user_block(&self, input: &str) {
        if let Some(ui) = &self.ui {
            best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::UserMessage(input.to_string())));
        } else {
            let user = self.theme.prompt;
            self.write_output(&format!("{user}>{user:#} {input}\n\n"));
        }
    }
}
