use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::Write;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};

pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

pub fn get_text() -> Result<Option<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_basic() {
        let _ = get_text();
    }
}
