use crate::error::{AppError, Result};
use std::process::Command;

pub fn open_url_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    Ok(())
}

#[cfg(feature = "ui")]
pub fn prompt_password(prompt: &str) -> Result<String> {
    inquire::Password::new(prompt)
        .with_display_mode(inquire::PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

#[cfg(not(feature = "ui"))]
pub fn prompt_password(prompt: &str) -> Result<String> {
    use std::io::BufRead;
    println!("{prompt}");
    let mut buffer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut buffer)
        .map_err(|e| AppError::Other(e.into()))?;
    Ok(buffer.trim_end_matches(&['\r', '\n'][..]).to_string())
}

#[cfg(feature = "ui")]
pub fn prompt_text(prompt: &str) -> Result<String> {
    inquire::Text::new(prompt)
        .prompt()
        .map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

#[cfg(not(feature = "ui"))]
pub fn prompt_text(prompt: &str) -> Result<String> {
    prompt_password(prompt)
}
