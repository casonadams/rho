use super::*;
use crate::tools::truncate::DEFAULT_MAX_BYTES;

async fn read_text(
    file_path: std::path::PathBuf,
    content: &str,
    (offset, limit): (Option<usize>, Option<usize>),
) -> ToolResult {
    let dir = file_path.parent().unwrap();
    tokio::fs::create_dir_all(dir).await.unwrap();
    tokio::fs::write(&file_path, content).await.unwrap();
    let result = ReadTool::new(dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset,
            limit,
        })
        .await
        .unwrap();
    let _ = tokio::fs::remove_dir_all(dir).await;
    result
}

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

fn numbered_lines(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn test_read_first_line_exceeds_byte_limit_points_at_bash() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(
        temp_dir.join("huge.txt"),
        &"x".repeat(DEFAULT_MAX_BYTES * 2),
        (None, None),
    )
    .await;

    assert!(!result.is_error);
    assert!(result.content.contains("[Line 1 is 100.0KB, exceeds 50.0KB limit."));
    assert!(result.content.contains("Use bash: sed -n '1p'"));
    assert!(result.content.contains("| head -c 51200]"));
}

#[tokio::test]
async fn test_read_byte_truncation_shows_range_and_next_offset() {
    // 1000 fixed-width lines: 51200 bytes fits 506 whole lines (51105 bytes).
    let content = vec!["x".repeat(100); 1000].join("\n");
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("wide.txt"), &content, (None, None)).await;

    assert!(!result.is_error);
    assert!(
        result
            .content
            .contains("[Showing lines 1-506 of 1000 (50.0KB limit). Use offset=507 to continue.]")
    );
    assert!(result.content.contains("     1\t"));
}

#[tokio::test]
async fn test_read_line_truncation_shows_continuation() {
    let content = numbered_lines(2500);
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("long.txt"), &content, (None, None)).await;

    assert!(!result.is_error);
    assert!(
        result
            .content
            .contains("[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]")
    );
    assert!(result.content.contains("2000\tline 2000"));
    assert!(!result.content.contains("2001\tline 2001"));
}

#[tokio::test]
async fn test_read_user_limit_reports_remaining_lines() {
    let content = numbered_lines(25);
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("medium.txt"), &content, (None, Some(10))).await;

    assert!(!result.is_error);
    assert!(
        result
            .content
            .contains("[15 more lines in file. Use offset=11 to continue.]")
    );
    assert!(result.content.contains("10\tline 10"));
    assert!(!result.content.contains("11\tline 11"));
}

#[tokio::test]
async fn test_read_user_limit_at_end_has_no_notice() {
    let content = numbered_lines(25);
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("exact.txt"), &content, (None, Some(25))).await;

    assert!(!result.is_error);
    assert!(result.content.contains("25\tline 25"));
    assert!(!result.content.contains("more lines in file"));
}

#[tokio::test]
async fn test_read_offset_continues_numbering() {
    let content = numbered_lines(2500);
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("long.txt"), &content, (Some(2001), None)).await;

    assert!(!result.is_error);
    assert!(result.content.contains("2001\tline 2001"));
    assert!(result.content.contains("2500\tline 2500"));
    assert!(!result.content.contains("[Showing lines"));
}

#[tokio::test]
async fn test_read_offset_beyond_end_of_file_errors() {
    let content = numbered_lines(5);
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("short.txt"), &content, (Some(100), None)).await;

    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("Offset 100 is beyond end of file (5 lines total)")
    );
}

#[tokio::test]
async fn test_read_empty_file_errors() {
    let temp_dir = std::env::temp_dir().join(format!("read_test_{}", uuid::Uuid::new_v4()));
    let result = read_text(temp_dir.join("empty.txt"), "", (None, None)).await;

    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("Offset 1 is beyond end of file (0 lines total)")
    );
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

// --- image attachment tests (generated fixtures, no binary files) ---

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{ImageFormat, Rgba, RgbaImage};
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
