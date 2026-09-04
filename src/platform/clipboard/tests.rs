use super::*;

#[test]
fn test_clipboard_basic() {
    let _ = get_text();
}

#[test]
fn test_save_image_to_temp_png() {
    let dummy = ClipboardImage {
        width: 2,
        height: 2,
        bytes: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255],
    };
    let path = save_image_to_temp_png(&dummy).unwrap();
    assert!(path.exists());
    let reader = image::ImageReader::open(&path).unwrap().decode().unwrap();
    assert_eq!(reader.width(), 2);
    assert_eq!(reader.height(), 2);
    let _ = std::fs::remove_file(path);
}
