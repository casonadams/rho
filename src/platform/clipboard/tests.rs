use super::*;

#[test]
fn test_clipboard_basic() {
    let _ = get_text();
    let _ = set_text("test");
    assert_eq!(get_text().unwrap(), Some("test".into()));
}

#[test]
fn test_save_image_to_temp_png() {
    let dummy = ClipboardImage {
        width: 2,
        height: 2,
        bytes: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255],
    };
    let path = save_image_to_temp_png(&dummy).unwrap();
    let _cleanup = rho_test_support::RemoveOnDrop(path.clone());
    assert!(path.exists());
    let reader = image::ImageReader::open(&path).unwrap().decode().unwrap();
    assert_eq!(reader.width(), 2);
    assert_eq!(reader.height(), 2);
}

#[tokio::test]
async fn test_save_image_to_temp_png_async() {
    let dummy = ClipboardImage {
        width: 2,
        height: 2,
        bytes: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255],
    };
    let path = save_image_to_temp_png_async(&dummy).await.unwrap();
    let _cleanup = rho_test_support::RemoveOnDrop(path.clone());
    assert!(path.exists());
    let reader = image::ImageReader::open(&path).unwrap().decode().unwrap();
    assert_eq!(reader.width(), 2);
    assert_eq!(reader.height(), 2);
}

#[tokio::test]
async fn test_clipboard_basic_async() {
    let _ = get_text_or_image_path_async().await;
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn pipe_to_command_reports_failed_exit() {
    assert!(pipe_to_command(&mut std::process::Command::new("false"), "text", CLIPBOARD_TIMEOUT).is_err());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn pipe_to_command_gives_up_on_a_command_that_hangs() {
    let mut hung = std::process::Command::new("sh");
    hung.args(["-c", "cat >/dev/null; exec sleep 5"]);
    let started = std::time::Instant::now();
    let err = pipe_to_command(&mut hung, "text", Duration::from_millis(80)).unwrap_err();
    assert!(err.to_string().contains("did not finish"), "{err:#}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn async_and_bounded_clipboard_calls_stay_in_memory_under_test() {
    set_text_async("from async".to_string()).await.unwrap();
    assert_eq!(get_text_or_image_path_bounded(), Some("from async".to_string()));
    assert_eq!(get_text_or_image_path_async().await, Some("from async".to_string()));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn pipe_to_command_reports_missing_program() {
    let mut cmd = std::process::Command::new("rho-no-such-clipboard-tool");
    assert!(pipe_to_command(&mut cmd, "text", CLIPBOARD_TIMEOUT).is_err());
}
