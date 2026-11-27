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
    assert!(pipe_to_command(&mut std::process::Command::new("false"), "text").is_err());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn pipe_to_command_reports_missing_program() {
    let mut cmd = std::process::Command::new("rho-no-such-clipboard-tool");
    assert!(pipe_to_command(&mut cmd, "text").is_err());
}
