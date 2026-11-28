use super::*;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size};
use test_case::test_case;

async fn read_text(
    file_path: std::path::PathBuf,
    content: &str,
    (offset, limit): (Option<usize>, Option<usize>),
) -> ToolResult {
    let dir = file_path.parent().unwrap();
    tokio::fs::create_dir_all(dir).await.unwrap();
    tokio::fs::write(&file_path, content).await.unwrap();
    ReadTool::new(dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset,
            limit,
            symbol: None,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn test_read_tool_happy_path() {
    let temp_dir = crate::test_support::TestDir::new();
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    let file_path = temp_dir.join("sample.txt");
    tokio::fs::write(&file_path, "line1\nline2\nline3\n").await.unwrap();

    let tool = ReadTool::new(&temp_dir);
    let res = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: Some(1),
            limit: Some(2),
            symbol: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("line1"));
    assert!(res.content.contains("line2"));
    assert!(!res.content.contains("line3"));
}

fn numbered_lines(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn test_read_oversized_line_shows_its_first_50kb() {
    let temp_dir = crate::test_support::TestDir::new();
    let line = format!(
        "{}{}",
        "x".repeat(DEFAULT_MAX_BYTES),
        "TAIL".repeat(DEFAULT_MAX_BYTES / 4)
    );
    let result = read_text(temp_dir.join("huge.txt"), &format!("{line}\nnext"), (None, None)).await;

    assert!(!result.is_error);
    assert!(
        result
            .content
            .starts_with(&format!("  1\t{}", "x".repeat(DEFAULT_MAX_BYTES)))
    );
    assert!(!result.content.contains("TAIL"));
    assert!(
        result
            .content
            .ends_with("[Line 1 is 100.0KB; showing its first 50.0KB. Use offset=2 to continue.]")
    );
}

#[tokio::test]
async fn test_read_oversized_line_reached_by_offset_cuts_on_char_boundary() {
    let temp_dir = crate::test_support::TestDir::new();
    let content = format!("short\n{}", "é".repeat(DEFAULT_MAX_BYTES));
    let result = read_text(temp_dir.join("wide.txt"), &content, (Some(2), None)).await;

    assert!(!result.is_error);
    let shown = result
        .content
        .strip_prefix("  2\t")
        .unwrap()
        .split('\n')
        .next()
        .unwrap();
    assert_eq!(shown, "é".repeat(DEFAULT_MAX_BYTES / 2));
    assert!(
        result
            .content
            .ends_with("[Line 2 is 100.0KB; showing its first 50.0KB.]")
    );
}

#[tokio::test]
async fn test_read_byte_truncation_shows_range_and_next_offset() {
    // 1000 fixed-width lines: 51200 bytes fits 506 whole lines (51105 bytes).
    let content = vec!["x".repeat(100); 1000].join("\n");
    let temp_dir = crate::test_support::TestDir::new();
    let result = read_text(temp_dir.join("wide.txt"), &content, (None, None)).await;

    assert!(!result.is_error);
    assert!(
        result
            .content
            .contains("[Showing lines 1-506 of 1000 (50.0KB limit). Use offset=507 to continue.]")
    );
    assert!(result.content.contains("  1\t"));
}

#[tokio::test]
async fn test_read_line_truncation_shows_continuation() {
    let content = numbered_lines(2500);
    let temp_dir = crate::test_support::TestDir::new();
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
    let temp_dir = crate::test_support::TestDir::new();
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
    let temp_dir = crate::test_support::TestDir::new();
    let result = read_text(temp_dir.join("exact.txt"), &content, (None, Some(25))).await;

    assert!(!result.is_error);
    assert!(result.content.contains("25\tline 25"));
    assert!(!result.content.contains("more lines in file"));
}

#[tokio::test]
async fn test_read_offset_continues_numbering() {
    let content = numbered_lines(2500);
    let temp_dir = crate::test_support::TestDir::new();
    let result = read_text(temp_dir.join("long.txt"), &content, (Some(2001), None)).await;

    assert!(!result.is_error);
    assert!(result.content.contains("2001\tline 2001"));
    assert!(result.content.contains("2500\tline 2500"));
    assert!(!result.content.contains("[Showing lines"));
}

#[tokio::test]
async fn test_read_offset_beyond_end_of_file_errors() {
    let content = numbered_lines(5);
    let temp_dir = crate::test_support::TestDir::new();
    let result = read_text(temp_dir.join("short.txt"), &content, (Some(100), None)).await;

    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("Offset 100 is beyond end of file (5 lines total)")
    );
}

#[test_case(None; "no offset")]
#[test_case(Some(5); "offset past end")]
#[tokio::test]
async fn test_read_empty_file_reports_empty(offset: Option<usize>) {
    let temp_dir = crate::test_support::TestDir::new();
    let result = read_text(temp_dir.join("empty.txt"), "", (offset, None)).await;
    assert!(!result.is_error);
    assert_eq!(result.content, "[File is empty]");
}

#[tokio::test]
async fn test_read_missing_file_suggests_close_paths() {
    let dir = crate::test_support::TestDir::new();
    std::fs::create_dir_all(dir.join("src/repl/input_reader")).unwrap();
    std::fs::write(dir.join("src/repl/input_reader/mod.rs"), "").unwrap();
    let tool = ReadTool::new(&dir);
    let res = tool
        .execute(ReadArgs {
            path: "src/repl/input_reader.rs".to_string(),
            offset: None,
            limit: None,
            symbol: None,
        })
        .await
        .unwrap();

    assert!(res.is_error);
    assert!(res.content.contains("File not found"));
    assert!(
        res.content.ends_with("\nDid you mean: src/repl/input_reader/mod.rs?"),
        "{}",
        res.content
    );
}

// --- image attachment tests (generated fixtures, no binary files) ---

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::{ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

async fn write_and_read(name: &str, bytes: &[u8]) -> ToolResult {
    let dir = crate::test_support::TestDir::new();
    let file_path = dir.join(name);
    tokio::fs::write(&file_path, bytes).await.unwrap();
    ReadTool::new(&dir)
        .execute(ReadArgs {
            path: file_path.to_string_lossy().into_owned(),
            offset: None,
            limit: None,
            symbol: None,
        })
        .await
        .unwrap()
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const IHDR_1X1_RGBA: [u8; 13] = [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];

fn solid_image(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(width, height, |_, _| Rgba([180, 60, 30, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, format).unwrap();
    buf.into_inner()
}

fn png_with_chunks(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = PNG_SIGNATURE.to_vec();
    for (chunk_type, data) in chunks {
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(*chunk_type);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&[0, 0, 0, 0]); // CRC not validated by the sniffer
    }
    bytes
}

#[tokio::test]
async fn test_read_png_attaches_inline_image() {
    let bytes = solid_image(ImageFormat::Png, 8, 8);
    let res = write_and_read("img.png", &bytes).await;

    assert!(!res.is_error);
    assert_eq!(res.content, "Read image file [image/png]");
    let image = res.image.expect("png read must attach an image");
    assert_eq!(image.mime, "image/png");
    assert_eq!(STANDARD.decode(&image.data).unwrap(), bytes);
}

#[tokio::test]
async fn test_read_bmp_converts_to_png_with_hint() {
    let res = write_and_read("img.bmp", &solid_image(ImageFormat::Bmp, 8, 8)).await;

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
    let bytes = png_with_chunks(&[
        (b"IHDR", &IHDR_1X1_RGBA),
        (b"IDAT", &[0xDE, 0xAD, 0xBE, 0xEF]),
        (b"IEND", &[]),
    ]);
    let res = write_and_read("img.png", &bytes).await;

    assert!(!res.is_error, "pi delivers the omission note as a successful result");
    assert_eq!(
        res.content,
        "Read image file [image/png]\n[Image omitted: could not be resized below the inline image size limit.]"
    );
    assert!(res.image.is_none());
}

#[tokio::test]
async fn test_read_apng_falls_back_to_binary_marker() {
    let bytes = png_with_chunks(&[
        (b"IHDR", &IHDR_1X1_RGBA),
        (b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]),
        (b"IDAT", &[0x01, 0x02, 0x03, 0x04]),
        (b"IEND", &[]),
    ]);
    let res = write_and_read("anim.png", &bytes).await;

    assert!(!res.is_error);
    assert!(res.content.contains("[Binary file:"));
    assert!(res.image.is_none());
}

#[tokio::test]
async fn test_read_oversized_file_spills_artifact() {
    let dir = crate::test_support::TestDir::new();
    let file_path = dir.join("large.txt");
    let content = vec!["x".repeat(100); 1000].join("\n");
    tokio::fs::write(&file_path, &content).await.unwrap();

    let tool = ReadTool::new(&dir);
    let res = tool
        .execute(ReadArgs {
            path: "large.txt".to_string(),
            offset: None,
            limit: None,
            symbol: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains(".rho/artifacts"));
    assert!(res.content.contains("[Output truncated. Full content"));

    let artifact_dir = dir.join(".rho/artifacts");
    let entries: Vec<_> = std::fs::read_dir(&artifact_dir).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let spilled_content = std::fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
    assert_eq!(spilled_content, content);
}

#[tokio::test]
async fn test_read_skill_full_content_on_first_read() {
    let dir = crate::test_support::TestDir::new();
    let skill_dir = dir.join(".agents/skills/custom");
    tokio::fs::create_dir_all(&skill_dir).await.unwrap();
    let file_path = skill_dir.join("SKILL.md");
    let content = numbered_lines(150);
    tokio::fs::write(&file_path, &content).await.unwrap();

    let tool = ReadTool::new(&dir);

    // First read with explicit limit of 10 should read all 150 lines
    let res = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: None,
            limit: Some(10),
            symbol: None,
        })
        .await
        .unwrap();

    assert!(!res.is_error);
    assert!(res.content.contains("line 1"));
    assert!(res.content.contains("line 150"));
    assert!(!res.content.contains("more lines in file"));

    // First read with offset 1 and limit 100 should also read all 150 lines
    let res_offset_1 = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: Some(1),
            limit: Some(100),
            symbol: None,
        })
        .await
        .unwrap();

    assert!(!res_offset_1.is_error);
    assert!(res_offset_1.content.contains("line 150"));

    // Subsequent read with offset > 1 should respect limit
    let res_page2 = tool
        .execute(ReadArgs {
            path: file_path.to_str().unwrap().to_string(),
            offset: Some(11),
            limit: Some(10),
            symbol: None,
        })
        .await
        .unwrap();

    assert!(!res_page2.is_error);
    assert!(res_page2.content.contains("line 11"));
    assert!(res_page2.content.contains("line 20"));
    assert!(!res_page2.content.contains("line 21"));
}

#[tokio::test]
async fn a_panicking_image_decoder_reports_an_image_error() {
    let result = process_image("photo.png", || panic!("decoder blew up")).await;

    assert!(result.is_error);
    assert!(
        result.content.starts_with("Failed to process image photo.png: "),
        "{}",
        result.content
    );
    assert!(result.content.contains("decoder blew up"), "{}", result.content);
}

async fn read_at(dir: &Path, name: &str, offset: Option<usize>, limit: Option<usize>) -> ToolResult {
    let args = ReadArgs {
        path: name.into(),
        offset,
        limit,
        symbol: None,
    };
    ReadTool::new(dir).execute(args).await.unwrap()
}

fn large_text_head() -> String {
    format!("{}\n", numbered_lines(300))
}

/// A text file larger than whole reads are allowed: 300 lines, then a sparse run of NULs.
fn large_text_file(dir: &Path, name: &str) -> u64 {
    let file = std::fs::File::create(dir.join(name)).unwrap();
    std::io::Write::write_all(&mut &file, large_text_head().as_bytes()).unwrap();
    let size = MAX_WHOLE_READ_BYTES * 4;
    file.set_len(size).unwrap();
    size
}

#[cfg(unix)]
#[tokio::test]
async fn a_fifo_is_refused_without_waiting_for_a_writer() {
    let dir = crate::test_support::TestDir::new();
    let fifo = std::ffi::CString::new(dir.join("pipe").to_str().unwrap()).unwrap();
    // SAFETY: `fifo` is a valid NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

    let result = read_at(&dir, "pipe", None, None).await;

    assert!(result.is_error);
    assert_eq!(result.content, "Cannot read pipe: not a regular file");
}

#[tokio::test]
async fn a_directory_is_refused_as_not_a_regular_file() {
    let dir = crate::test_support::TestDir::new();
    std::fs::create_dir(dir.join("sub")).unwrap();
    let result = read_at(&dir, "sub", None, None).await;
    assert!(result.is_error);
    assert_eq!(result.content, "Cannot read sub: it is a directory");
}

#[tokio::test]
async fn a_large_binary_file_is_described_from_its_first_bytes() {
    let dir = crate::test_support::TestDir::new();
    let size = 1 << 30;
    std::fs::File::create(dir.join("disk.img"))
        .unwrap()
        .set_len(size)
        .unwrap();

    let result = read_at(&dir, "disk.img", None, None).await;

    assert!(!result.is_error);
    assert_eq!(result.content, format!("[Binary file: {size} bytes, path: disk.img]"));
}

#[tokio::test]
async fn a_large_text_file_shows_the_requested_window() {
    let dir = crate::test_support::TestDir::new();
    let size = format_size(large_text_file(&dir, "big.log") as usize);
    let cases = [
        (
            Some(2),
            Some(2),
            "  2\tline 2\n  3\tline 3\n\n\n[More lines in file. Use offset=4 to continue.]".to_string(),
        ),
        (
            Some(299),
            None,
            format!(
                "299\tline 299\n300\tline 300\n\n\n[Showing lines 299-300 of a {size} file. Use offset=301 to continue.]"
            ),
        ),
    ];
    for (offset, limit, expected) in cases {
        let result = read_at(&dir, "big.log", offset, limit).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.content, expected);
    }
}

#[tokio::test]
async fn a_large_text_files_oversized_line_and_end_are_reported() {
    let dir = crate::test_support::TestDir::new();
    let size = large_text_file(&dir, "big.log");

    let tail = read_at(&dir, "big.log", Some(301), None).await;
    let shown = format!("301\t{}\n", "\0".repeat(DEFAULT_MAX_BYTES));
    let note = format!(
        "[Line 301 is {}; showing its first {}.]",
        format_size(size as usize - large_text_head().len()),
        format_size(DEFAULT_MAX_BYTES)
    );
    assert!(!tail.is_error);
    assert_eq!(tail.content, format!("{shown}\n{note}"));

    let beyond = read_at(&dir, "big.log", Some(303), None).await;
    assert!(beyond.is_error);
    assert_eq!(beyond.content, "Offset 303 is beyond end of file (301 lines total)");
}

#[test_case(Some(2), None; "with offset")]
#[test_case(None, Some(5); "with limit")]
#[tokio::test]
async fn symbol_cannot_be_combined_with_offset_or_limit(offset: Option<usize>, limit: Option<usize>) {
    let dir = crate::test_support::TestDir::new();
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    let args = ReadArgs {
        path: "a.rs".into(),
        offset,
        limit,
        symbol: Some("a".into()),
    };
    let result = ReadTool::new(&dir).execute(args).await.unwrap();
    assert!(result.is_error);
    assert!(
        result.content.contains("symbol cannot be combined"),
        "{}",
        result.content
    );
}
