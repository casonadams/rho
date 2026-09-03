use super::*;

#[tokio::test]
async fn test_read_tool_happy_path() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    tokio::fs::write(&file_path, "line1\nline2\nline3\n").await.unwrap();

    let tool = ReadTool::new(&temp_dir);
    let res = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: Some(1),
            limit: Some(2),
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("line1"));
    assert!(res.content.contains("line2"));
    assert!(!res.content.contains("line3"));

    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_read_truncates_at_byte_limit() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("large.txt");
    tokio::fs::write(&file_path, "x".repeat(MAX_READ_BYTES * 2))
        .await
        .unwrap();

    let result = ReadTool::new(&temp_dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("Truncated at 50 KB limit"));
    assert!(result.content.len() <= MAX_READ_BYTES + 200);
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_read_missing_file() {
    let tool = ReadTool::new(std::env::temp_dir());
    let res = tool
        .execute(ReadArgs {
            path: "nonexistent_file_xyz_123.txt".to_string(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("File not found"));
}

#[tokio::test]
async fn test_read_builtin_embedded_skill() {
    let tool = ReadTool::new(std::env::temp_dir());
    let res = tool
        .execute(ReadArgs {
            path: "rho://skills/create-plugin".to_string(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("Creating an MCP Tool Server"));
    assert!(res.content.contains("Scaffold and package"));
}

// --- image attachment tests (generated fixtures, no binary files) ---

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{ExtendedColorType, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

async fn write_and_read(dir: std::path::PathBuf, name: &str, bytes: &[u8]) -> ToolResult {
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let file_path = dir.join(name);
    tokio::fs::write(&file_path, bytes).await.unwrap();
    let result = ReadTool::new(&dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset: None,
            limit: None,
        })
        .await
        .unwrap();
    let _ = tokio::fs::remove_dir_all(dir).await;
    result
}

fn solid_png(width: u32, height: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(width, height, |_, _| Rgba([180, 60, 30, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

fn solid_bmp(width: u32, height: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(width, height, |_, _| Rgba([180, 60, 30, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Bmp).unwrap();
    buf.into_inner()
}

fn png_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(chunk_type);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&[0, 0, 0, 0]); // CRC not validated by the sniffer
    chunk
}

#[tokio::test]
async fn test_read_png_attaches_inline_image() {
    let bytes = solid_png(8, 8);
    let res = write_and_read(
        std::env::temp_dir().join(format!("read_img_{}", uuid::Uuid::new_v4())),
        "img.png",
        &bytes,
    )
    .await;

    assert!(!res.is_error);
    assert_eq!(res.content, "Read image file [image/png]");
    let image = res.image.expect("png read must attach an image");
    assert_eq!(image.mime, "image/png");
    assert_eq!(STANDARD.decode(&image.data).unwrap(), bytes);
}

#[tokio::test]
async fn test_read_bmp_converts_to_png_with_hint() {
    let res = write_and_read(
        std::env::temp_dir().join(format!("read_bmp_{}", uuid::Uuid::new_v4())),
        "img.bmp",
        &solid_bmp(8, 8),
    )
    .await;

    assert!(!res.is_error);
    assert_eq!(
        res.content,
        "Read image file [image/png]\n[Image converted from image/bmp to image/png.]"
    );
    let image = res.image.expect("converted bmp must attach an image");
    assert_eq!(image.mime, "image/png");
    let decoded = STANDARD.decode(&image.data).unwrap();
    assert!(decoded.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
}

#[tokio::test]
async fn test_read_corrupt_image_reports_omission_without_block() {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
    bytes.extend(png_chunk(b"IDAT", &[0xDE, 0xAD, 0xBE, 0xEF]));
    bytes.extend(png_chunk(b"IEND", &[]));
    let res = write_and_read(
        std::env::temp_dir().join(format!("read_corrupt_{}", uuid::Uuid::new_v4())),
        "img.png",
        &bytes,
    )
    .await;

    assert!(!res.is_error, "pi delivers the omission note as a successful result");
    assert_eq!(
        res.content,
        "Read image file [image/png]\n[Image omitted: could not be resized below the inline image size limit.]"
    );
    assert!(res.image.is_none());
}

#[tokio::test]
async fn test_read_apng_falls_back_to_binary_marker() {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
    bytes.extend(png_chunk(b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]));
    bytes.extend(png_chunk(b"IDAT", &[0x01, 0x02, 0x03, 0x04]));
    bytes.extend(png_chunk(b"IEND", &[]));
    let res = write_and_read(
        std::env::temp_dir().join(format!("read_apng_{}", uuid::Uuid::new_v4())),
        "anim.png",
        &bytes,
    )
    .await;

    assert!(!res.is_error);
    assert!(res.content.contains("[Binary file:"));
    assert!(res.image.is_none());
}
