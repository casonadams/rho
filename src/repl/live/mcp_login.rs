use async_trait::async_trait;
use crossterm::event::{Event, KeyEventKind};
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};
use rho_harness_core::config::Config;
use tokio::sync::mpsc;

use crate::error::{AppError, Result};
use crate::repl::live::types::TerminalInputReader;
use crate::ui::interactive::{InputAction, TerminalBackend, TerminalController, TranscriptItem, map_key};
use crate::ui::render::NoticeKind;
use crate::ui::render::notices::format_notice;
use crate::ui::theme::Theme;

type Notices = mpsc::UnboundedSender<String>;
pub(crate) type UrlOpener = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

pub(crate) fn system_browser() -> UrlOpener {
    std::sync::Arc::new(|url: &str| {
        let url = url.to_string();
        tokio::spawn(async move {
            let _ = crate::cli::auth::open_url_in_browser_async(&url).await;
        });
    })
}

struct TranscriptCallbacks {
    notices: Notices,
    open_url: UrlOpener,
}

#[async_trait]
impl OAuthLoginCallbacks for TranscriptCallbacks {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()> {
        let heading = instructions.unwrap_or("Authorize in your browser");
        crate::ui::best_effort(self.notices.send(format!("{heading}:\n{url}\nPress Esc to cancel.")));
        (self.open_url)(url);
        Ok(())
    }

    async fn on_device_code(&self, _info: &DeviceCodeInfo<'_>) -> Result<()> {
        Err(AppError::Auth("MCP sign-in does not use device codes".to_string()))
    }

    async fn on_prompt(&self, _message: &str, _secret: bool) -> Result<String> {
        Err(AppError::Auth("MCP sign-in does not prompt for input".to_string()))
    }

    async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
        Err(AppError::Auth("MCP sign-in does not prompt for a choice".to_string()))
    }

    async fn on_progress(&self, message: &str) -> Result<()> {
        crate::ui::best_effort(self.notices.send(message.to_string()));
        Ok(())
    }
}

pub(crate) struct McpLoginUi<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub input: &'a mut TerminalInputReader,
    pub theme: &'a Theme,
    pub open_url: UrlOpener,
}

impl<B: TerminalBackend> McpLoginUi<'_, B> {
    fn show(&mut self, kind: NoticeKind, text: &str) -> Result<()> {
        let notice = format_notice(self.theme, kind, text);
        self.controller.push_transcript_item(TranscriptItem::Notice(notice))?;
        self.controller.redraw()?;
        Ok(())
    }

    fn handle_input(&mut self, event: Option<std::io::Result<Event>>) -> Result<bool> {
        let Some(event) = event else {
            return Err(anyhow::anyhow!("Terminal input reader stopped").into());
        };
        match event? {
            Event::Key(key) if key.kind != KeyEventKind::Release => Ok(map_key(key) == InputAction::Cancel),
            Event::Resize(..) | Event::FocusGained => {
                self.controller.refresh_size()?;
                self.controller.redraw()?;
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

pub(crate) async fn run_mcp_login<B: TerminalBackend>(
    ui: &mut McpLoginUi<'_, B>,
    server: &str,
    config: &Config,
) -> Result<()> {
    let (tx, mut notices) = mpsc::unbounded_channel();
    let callbacks = TranscriptCallbacks {
        notices: tx,
        open_url: std::sync::Arc::clone(&ui.open_url),
    };
    let login = crate::cli::mcp::login_mcp_server_with(server, config, &callbacks);
    tokio::pin!(login);
    let result = loop {
        tokio::select! {
            biased;
            Some(text) = notices.recv() => ui.show(NoticeKind::Info, &text)?,
            event = ui.input.recv() => {
                if ui.handle_input(event)? {
                    break Err(AppError::Cancelled(format!("MCP sign-in to '{server}'")));
                }
            }
            res = &mut login => break res,
        }
    };
    while let Ok(text) = notices.try_recv() {
        ui.show(NoticeKind::Info, &text)?;
    }
    match &result {
        Ok(()) => ui.show(NoticeKind::Info, &format!("Signed in to MCP server '{server}'."))?,
        Err(AppError::Cancelled(_)) => ui.show(NoticeKind::Warn, &format!("Sign-in to '{server}' cancelled."))?,
        Err(e) => ui.show(NoticeKind::Warn, &format!("Sign-in to '{server}' failed: {e}"))?,
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::NullBackend;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn remote_config(url: &str) -> (Config, rho_test_support::TestDir) {
        let dir = rho_test_support::TestDir::new();
        let mut config = Config {
            config_dir: dir.to_path_buf(),
            ..Default::default()
        };
        let server = rho_harness_core::config::McpServerConfig {
            url: Some(url.to_string()),
            enabled: true,
            ..Default::default()
        };
        config.mcp.servers.insert("srv".into(), server);
        (config, dir)
    }

    fn transcript(controller: &TerminalController<NullBackend>) -> String {
        controller
            .transcript()
            .iter()
            .filter_map(|item| match item {
                TranscriptItem::Notice(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn no_browser() -> UrlOpener {
        std::sync::Arc::new(|_: &str| {})
    }

    fn reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nWWW-Authenticate: Bearer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    async fn oauth_server() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_string();
                let response = match path.as_str() {
                    "/mcp" => reply("401 Unauthorized", ""),
                    "/register" => reply("201 Created", r#"{"client_id":"cid"}"#),
                    p if p.contains("oauth-protected-resource") => {
                        reply("200 OK", &format!(r#"{{"authorization_servers":["http://{addr}"]}}"#))
                    }
                    _ => reply(
                        "200 OK",
                        &format!(
                            r#"{{"issuer":"http://{addr}","authorization_endpoint":"http://{addr}/authorize","token_endpoint":"http://{addr}/token","registration_endpoint":"http://{addr}/register"}}"#
                        ),
                    ),
                };
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{addr}/mcp")
    }

    #[tokio::test]
    async fn esc_cancels_a_pending_sign_in_and_reports_it_in_the_transcript() {
        let (config, _dir) = remote_config(&oauth_server().await);
        let mut controller = crate::repl::test_controller();
        let url_shown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let esc = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let mut input = TerminalInputReader::spawn_gated(esc, std::sync::Arc::clone(&url_shown));
        let theme = Theme::default();
        let mut ui = McpLoginUi {
            controller: &mut controller,
            input: &mut input,
            theme: &theme,
            open_url: std::sync::Arc::new(move |_: &str| url_shown.store(true, std::sync::atomic::Ordering::SeqCst)),
        };

        let result = run_mcp_login(&mut ui, "srv", &config).await;

        assert!(matches!(result, Err(AppError::Cancelled(_))), "{result:?}");
        let text = transcript(&controller);
        assert!(text.contains("Discovering how to authorize MCP server 'srv'"), "{text}");
        assert!(text.contains("/authorize?"), "{text}");
        assert!(text.contains("Sign-in to 'srv' cancelled."), "{text}");
    }

    #[tokio::test]
    async fn failures_are_reported_without_leaving_the_tui() {
        let (config, _dir) = remote_config("http://127.0.0.1:9/mcp");
        let mut controller = crate::repl::test_controller();
        let mut input = TerminalInputReader::spawn_dummy();
        let theme = Theme::default();
        let mut ui = McpLoginUi {
            controller: &mut controller,
            input: &mut input,
            theme: &theme,
            open_url: no_browser(),
        };

        let result = run_mcp_login(&mut ui, "srv", &config).await;

        assert!(result.is_err());
        assert!(transcript(&controller).contains("Sign-in to 'srv' failed:"));
    }
}
