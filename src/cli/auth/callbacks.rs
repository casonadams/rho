use super::terminal::{open_url_in_browser_async, prompt_password, prompt_text};
use crate::error::{AppError, Result};
use async_trait::async_trait;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};
use std::io::{BufRead, Write};

pub struct TerminalOAuthCallbacks;

fn write_select_options<W: Write>(w: &mut W, message: &str, options: &[SelectOption]) -> std::io::Result<()> {
    writeln!(w, "\n{message}")?;
    for (idx, opt) in options.iter().enumerate() {
        if let Some(desc) = &opt.description {
            writeln!(w, "  {}. {} - {desc}", idx + 1, opt.label)?;
        } else {
            writeln!(w, "  {}. {}", idx + 1, opt.label)?;
        }
    }
    write!(w, "Enter selection [1-{}]: ", options.len())?;
    w.flush()
}

fn parse_selection(input: &str, options: &[SelectOption]) -> Option<String> {
    let idx = input.trim().parse::<usize>().ok()?;
    if (1..=options.len()).contains(&idx) {
        Some(options[idx - 1].id.clone())
    } else {
        None
    }
}

fn prompt_ui_select_from<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    message: &str,
    options: &[SelectOption],
) -> Result<Option<String>> {
    if options.is_empty() {
        return Ok(None);
    }
    write_select_options(writer, message, options).map_err(|e| AppError::Other(e.into()))?;
    let mut input = String::new();
    reader.read_line(&mut input).map_err(|e| AppError::Other(e.into()))?;
    Ok(parse_selection(&input, options))
}

async fn prompt_ui_select(message: &str, options: &[SelectOption]) -> Result<Option<String>> {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    prompt_ui_select_from(&mut stdin, &mut stdout, message, options)
}

#[async_trait]
impl OAuthLoginCallbacks for TerminalOAuthCallbacks {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()> {
        let msg = instructions.unwrap_or("Authenticate in your browser:");
        println!("\n  \x1b[1m{msg}\x1b[0m");
        println!("  URL: \x1b[4;34m{url}\x1b[0m\n");
        let _ = open_url_in_browser_async(url).await;
        Ok(())
    }

    async fn on_device_code(&self, info: &DeviceCodeInfo<'_>) -> Result<()> {
        println!(
            "\n  \x1b[1mFirst copy your one-time code:\x1b[0m \x1b[1;36m{}\x1b[0m",
            info.user_code
        );
        println!(
            "  \x1b[1mThen open:\x1b[0m \x1b[4;34m{}\x1b[0m\n",
            info.verification_uri
        );
        let _ = open_url_in_browser_async(info.verification_uri).await;
        Ok(())
    }

    async fn on_prompt(&self, message: &str, secret: bool) -> Result<String> {
        let msg = message.to_string();
        tokio::task::spawn_blocking(move || {
            if secret {
                prompt_password(&msg)
            } else {
                prompt_text(&msg)
            }
        })
        .await
        .map_err(|e| AppError::Other(e.into()))?
    }

    async fn on_select(&self, message: &str, options: &[SelectOption]) -> Result<Option<String>> {
        prompt_ui_select(message, options).await
    }

    async fn on_progress(&self, message: &str) -> Result<()> {
        println!("  • {message}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_prompt_ui_select_from_empty_options() {
        let mut reader = Cursor::new(b"1\n");
        let mut writer = Vec::new();
        let res = prompt_ui_select_from(&mut reader, &mut writer, "Select one", &[]).unwrap();
        assert_eq!(res, None);
        assert!(writer.is_empty());
    }

    #[test]
    fn test_prompt_ui_select_from_valid_choices() {
        let options = vec![
            SelectOption {
                id: "first".into(),
                label: "Option 1".into(),
                description: Some("First desc".into()),
            },
            SelectOption {
                id: "second".into(),
                label: "Option 2".into(),
                description: None,
            },
        ];

        let mut reader = Cursor::new(b"1\n");
        let mut writer = Vec::new();
        let res = prompt_ui_select_from(&mut reader, &mut writer, "Choose option", &options).unwrap();
        assert_eq!(res, Some("first".to_string()));
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("Choose option"));
        assert!(output.contains("1. Option 1 - First desc"));
        assert!(output.contains("2. Option 2"));

        let mut reader2 = Cursor::new(b"2\n");
        let mut writer2 = Vec::new();
        let res2 = prompt_ui_select_from(&mut reader2, &mut writer2, "Choose option", &options).unwrap();
        assert_eq!(res2, Some("second".to_string()));
    }

    #[test]
    fn test_prompt_ui_select_from_invalid_inputs() {
        let options = vec![SelectOption {
            id: "opt".into(),
            label: "Only Option".into(),
            description: None,
        }];

        let mut reader_out_of_bounds = Cursor::new(b"0\n");
        let mut writer = Vec::new();
        assert_eq!(
            prompt_ui_select_from(&mut reader_out_of_bounds, &mut writer, "Choose", &options).unwrap(),
            None
        );

        let mut reader_high = Cursor::new(b"99\n");
        let mut writer2 = Vec::new();
        assert_eq!(
            prompt_ui_select_from(&mut reader_high, &mut writer2, "Choose", &options).unwrap(),
            None
        );

        let mut reader_text = Cursor::new(b"abc\n");
        let mut writer3 = Vec::new();
        assert_eq!(
            prompt_ui_select_from(&mut reader_text, &mut writer3, "Choose", &options).unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn test_terminal_oauth_callbacks_progress() {
        let callbacks = TerminalOAuthCallbacks;
        assert!(callbacks.on_progress("Connecting...").await.is_ok());
    }
}
