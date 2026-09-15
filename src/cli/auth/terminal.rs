use crate::error::{AppError, Result};
use std::io::IsTerminal;

#[cfg(target_os = "windows")]
fn open_browser_windows(url: &str) -> std::io::Result<()> {
    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Start-Process -FilePath $env:RHO_AUTH_URL",
    ])
    .env("RHO_AUTH_URL", url);

    if cmd.spawn().is_err() {
        let mut fallback = tokio::process::Command::new("cmd");
        fallback.args(["/C", "start", "", url.replace('&', "^&").as_str()]);
        fallback.spawn()?;
    }
    Ok(())
}

pub async fn open_url_in_browser_async(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    tokio::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "linux")]
    tokio::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    open_browser_windows(url)?;
    Ok(())
}

pub fn prompt_password(prompt: &str) -> Result<String> {
    use std::io::Write;
    print!("{prompt} ");
    std::io::stdout().flush().map_err(|e| AppError::Other(e.into()))?;

    if !std::io::stdin().is_terminal() {
        let mut buffer = String::new();
        std::io::stdin()
            .read_line(&mut buffer)
            .map_err(|e| AppError::Other(e.into()))?;
        return Ok(buffer.trim_end_matches(&['\r', '\n'][..]).to_string());
    }

    crossterm::terminal::enable_raw_mode().map_err(|e| AppError::Other(e.into()))?;
    let mut password = String::new();
    let res = loop {
        match crossterm::event::read() {
            Ok(crossterm::event::Event::Key(key)) => match key.code {
                crossterm::event::KeyCode::Enter => break Ok(password),
                crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('d')
                    if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    break Err(AppError::Cancelled("Input cancelled".to_string()));
                }
                crossterm::event::KeyCode::Char(c) => {
                    password.push(c);
                    print!("*");
                    let _ = std::io::stdout().flush();
                }
                crossterm::event::KeyCode::Backspace if !password.is_empty() => {
                    password.pop();
                    print!("\x08 \x08");
                    let _ = std::io::stdout().flush();
                }
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(AppError::Other(e.into())),
        }
    };
    let _ = crossterm::terminal::disable_raw_mode();
    println!();
    res
}

pub fn prompt_text(prompt: &str) -> Result<String> {
    use std::io::Write;
    print!("{prompt} ");
    std::io::stdout().flush().map_err(|e| AppError::Other(e.into()))?;
    let mut buffer = String::new();
    std::io::stdin()
        .read_line(&mut buffer)
        .map_err(|e| AppError::Other(e.into()))?;
    let trimmed = buffer.trim_end_matches(&['\r', '\n'][..]).to_string();
    if trimmed.is_empty() && !std::io::stdin().is_terminal() {
        return Err(AppError::Cancelled("Input cancelled".to_string()));
    }
    Ok(trimmed)
}

pub fn prompt_select<T: std::fmt::Display>(message: &str, items: &[T]) -> Result<usize> {
    use std::io::Write;
    println!("{message}");
    for (idx, item) in items.iter().enumerate() {
        println!("  {}. {}", idx + 1, item);
    }
    loop {
        print!("Enter choice (1-{}): ", items.len());
        std::io::stdout().flush().map_err(|e| AppError::Other(e.into()))?;
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| AppError::Other(e.into()))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AppError::Cancelled("Selection cancelled".to_string()));
        }
        if let Ok(num) = trimmed.parse::<usize>()
            && num >= 1
            && num <= items.len()
        {
            return Ok(num - 1);
        }
        println!("Invalid choice. Please enter a number between 1 and {}.", items.len());
    }
}
