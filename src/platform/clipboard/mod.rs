use anyhow::{Context, Result};
use rho_harness_core::locking::lock;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::Write;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(test)]
mod tests;

/// How long the UI waits on the clipboard, or a clipboard command, before giving up.
pub const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(1);

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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_command_text(cmd: Command) -> Option<String> {
    let output = crate::platform::git::run_command_with_timeout(cmd, CLIPBOARD_TIMEOUT)?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

#[cfg(target_os = "macos")]
fn get_os_clipboard_text() -> Option<String> {
    read_command_text(Command::new("pbpaste"))
}

#[cfg(target_os = "linux")]
fn get_os_clipboard_text() -> Option<String> {
    let mut xclip = Command::new("xclip");
    xclip.args(["-selection", "clipboard", "-o"]);
    read_command_text(Command::new("wl-paste")).or_else(|| read_command_text(xclip))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn get_os_clipboard_text() -> Option<String> {
    None
}

static TEST_CLIPBOARD_TEXT: Mutex<Option<String>> = Mutex::new(None);

pub fn get_text() -> Result<Option<String>> {
    if cfg!(test) {
        return Ok(lock(&TEST_CLIPBOARD_TEXT).clone());
    }
    let _single_flight = lock(&CLIPBOARD_LOCK);
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && let Ok(text) = clipboard.get_text()
        && !text.is_empty()
    {
        return Ok(Some(text));
    }

    Ok(get_os_clipboard_text())
}

pub fn get_text_or_image_path() -> Option<String> {
    if let Ok(Some(img)) = get_image()
        && let Ok(path) = save_image_to_temp_png(&img)
    {
        return Some(path.to_string_lossy().into_owned());
    }
    get_text().ok().flatten()
}

/// The clipboard's image (saved as a PNG path) or text, read off the UI thread; `None` when it holds
/// neither or does not answer within `CLIPBOARD_TIMEOUT`.
pub async fn get_text_or_image_path_async() -> Option<String> {
    tokio::time::timeout(CLIPBOARD_TIMEOUT, read_text_or_image_path())
        .await
        .ok()
        .flatten()
}

async fn read_text_or_image_path() -> Option<String> {
    if let Ok(Some(img)) = get_image_async().await
        && let Ok(path) = save_image_to_temp_png_async(&img).await
    {
        return Some(path.to_string_lossy().into_owned());
    }
    tokio::task::spawn_blocking(get_text).await.ok()?.ok()?
}

/// Like `get_text_or_image_path_async`, for synchronous key handlers: reads on another thread and
/// waits at most `CLIPBOARD_TIMEOUT` for it.
pub fn get_text_or_image_path_bounded() -> Option<String> {
    if cfg!(test) {
        return get_text().ok().flatten();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(get_text_or_image_path());
    });
    rx.recv_timeout(CLIPBOARD_TIMEOUT).ok().flatten()
}

/// Puts `text` on the clipboard off the UI thread, failing when it takes over `CLIPBOARD_TIMEOUT`.
pub async fn set_text_async(text: String) -> Result<()> {
    if cfg!(test) {
        return set_text(&text);
    }
    tokio::time::timeout(CLIPBOARD_TIMEOUT, tokio::task::spawn_blocking(move || set_text(&text)))
        .await
        .context("the clipboard did not answer in time")?
        .context("clipboard task failed")?
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pipe_to_command(cmd: &mut Command, text: &str, timeout: Duration) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {program}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("failed to write clipboard text to {program}"))?;
    }
    let status = crate::platform::git::wait_for_child(&mut child, timeout)
        .with_context(|| format!("{program} did not finish within {timeout:?}"))?;
    anyhow::ensure!(status.success(), "{program} exited with {status}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_os_clipboard_text(text: &str) -> Result<()> {
    pipe_to_command(&mut Command::new("pbcopy"), text, CLIPBOARD_TIMEOUT)
}

#[cfg(target_os = "linux")]
fn set_os_clipboard_text(text: &str) -> Result<()> {
    pipe_to_command(&mut Command::new("wl-copy"), text, CLIPBOARD_TIMEOUT).or_else(|wl_err| {
        let mut xclip = Command::new("xclip");
        xclip.args(["-selection", "clipboard"]);
        pipe_to_command(&mut xclip, text, CLIPBOARD_TIMEOUT).with_context(|| format!("wl-copy also failed: {wl_err:#}"))
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn set_os_clipboard_text(_text: &str) -> Result<()> {
    anyhow::bail!("no OS clipboard command is available on this platform")
}

pub fn set_text(text: &str) -> Result<()> {
    if cfg!(test) {
        *lock(&TEST_CLIPBOARD_TEXT) = Some(text.to_string());
        return Ok(());
    }
    let _single_flight = lock(&CLIPBOARD_LOCK);
    if let Ok(mut clipboard) = arboard::Clipboard::new()
        && clipboard.set_text(text).is_ok()
    {
        return Ok(());
    }

    set_os_clipboard_text(text)
}

pub fn get_image() -> Result<Option<ClipboardImage>> {
    if cfg!(test) {
        return Ok(None);
    }
    let _single_flight = lock(&CLIPBOARD_LOCK);
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
    let write_fn = || {
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
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(write_fn)
    } else {
        write_fn()
    }
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
