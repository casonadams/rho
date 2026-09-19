use crate::error::{AppError, Result};

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
    print!("{prompt}: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    rpassword::read_password().map_err(|_| AppError::Cancelled("Input cancelled".to_string()))
}

pub fn prompt_text(prompt: &str) -> Result<String> {
    use std::io::BufRead;
    print!("{prompt}: ");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let mut buffer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut buffer)
        .map_err(|e| AppError::Other(e.into()))?;
    Ok(buffer.trim_end_matches(&['\r', '\n'][..]).to_string())
}
