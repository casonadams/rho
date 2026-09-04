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

pub fn get_text() -> Result<Option<String>> {
    let _single_flight = CLIPBOARD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(text) = clipboard.get_text()
        && !text.is_empty()
    {
        return Ok(Some(text));
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("pbpaste").output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("wl-paste").output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
        if let Ok(output) = Command::new("xclip").args(["-selection", "clipboard", "-o"]).output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
    }

    Ok(None)
}

pub fn set_text(text: &str) -> Result<()> {
    let _single_flight = CLIPBOARD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && clipboard.set_text(text).is_ok()
    {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(mut child) = Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
        if let Ok(mut child) = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
    }

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
