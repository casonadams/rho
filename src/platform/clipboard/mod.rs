use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::Write;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};
use std::sync::Mutex;

#[cfg(test)]
mod tests;

pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

/// AppKit's NSPasteboard (arboard's macOS backend) segfaults when accessed
/// from multiple threads at once. Production only reaches the clipboard from
/// the UI thread; serialize access so parallel tests and future callers get
/// that same single-flight behavior.
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

#[cfg(target_os = "macos")]
fn get_os_clipboard_text() -> Option<String> {
    if let Ok(output) = Command::new("pbpaste").output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn get_os_clipboard_text() -> Option<String> {
    let mut wl = Command::new("wl-paste");
    let mut xclip = Command::new("xclip");
    xclip.args(["-selection", "clipboard", "-o"]);
    for cmd in [&mut wl, &mut xclip] {
        if let Ok(output) = cmd.output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_os_clipboard_text() -> Option<String> {
    None
}

pub fn get_text() -> Result<Option<String>> {
    let _single_flight = CLIPBOARD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(text) = clipboard.get_text()
        && !text.is_empty()
    {
        return Ok(Some(text));
    }

    Ok(get_os_clipboard_text())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pipe_to_command(cmd: &mut Command, text: &str) -> bool {
    if let Ok(mut child) = cmd.stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return child.wait().is_ok();
    }
    false
}

fn set_os_clipboard_text(text: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = pipe_to_command(&mut Command::new("pbcopy"), text);
    }

    #[cfg(target_os = "linux")]
    {
        if !pipe_to_command(&mut Command::new("wl-copy"), text) {
            let mut cmd = Command::new("xclip");
            cmd.args(["-selection", "clipboard"]);
            let _ = pipe_to_command(&mut cmd, text);
        }
    }
}

pub fn set_text(text: &str) -> Result<()> {
    let _single_flight = CLIPBOARD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && clipboard.set_text(text).is_ok()
    {
        return Ok(());
    }

    set_os_clipboard_text(text);
    Ok(())
}

pub fn get_image() -> Result<Option<ClipboardImage>> {
    let _single_flight = CLIPBOARD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(img) = clipboard.get_image()
    {
        return Ok(Some(ClipboardImage {
            width: img.width,
            height: img.height,
            bytes: img.bytes.into_owned(),
        }));
    }
    Ok(None)
}

pub async fn get_image_async() -> Result<Option<ClipboardImage>> {
    tokio::task::spawn_blocking(get_image)
        .await
        .map_err(|e| anyhow::anyhow!("Clipboard image task failed: {e}"))?
}

pub fn save_image_to_temp_png(img: &ClipboardImage) -> Result<std::path::PathBuf> {
    let file_name = format!("rho-clipboard-{}.png", uuid::Uuid::new_v4());
    let path = std::env::temp_dir().join(file_name);
    image::save_buffer_with_format(
        &path,
        &img.bytes,
        img.width as u32,
        img.height as u32,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    Ok(path)
}

pub async fn save_image_to_temp_png_async(img: &ClipboardImage) -> Result<std::path::PathBuf> {
    let bytes = img.bytes.clone();
    let width = img.width as u32;
    let height = img.height as u32;
    tokio::task::spawn_blocking(move || {
        let file_name = format!("rho-clipboard-{}.png", uuid::Uuid::new_v4());
        let path = std::env::temp_dir().join(file_name);
        image::save_buffer_with_format(
            &path,
            &bytes,
            width,
            height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        Ok(path)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Image save task failed: {e}"))?
}
