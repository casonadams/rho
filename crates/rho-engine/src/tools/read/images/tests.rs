use super::ProcessError;
use super::dimension_note;
use super::process_image;
use super::resize::{MAX_BASE64_BYTES, ResizeLimits, resize_with_limits, shrink};
use super::sniff::{SniffedMime, detect_supported_image_mime};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, Rgba};
use std::io::Cursor;

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn solid_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(width, height, |_, _| Rgba([180, 60, 30, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

fn noise_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        let v = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA6B);
        Rgba([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255])
    });
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::NoFilter)
        .write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgba8)
        .unwrap();
    buf
}

fn gif_fixture() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(4, 4, |_, _| Rgba([10, 200, 90, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Gif).unwrap();
    buf.into_inner()
}

/// Real 2×2 24-bit BMP with controllable planes/bpp for header-sanity tests.
fn bmp_with(planes: u16, bits_per_pixel: u16) -> Vec<u8> {
    let row_size = (2 * usize::from(bits_per_pixel)).div_ceil(32) * 4;
    let pixel_data = row_size * 2;
    let pixel_offset = 14u32 + 40;
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(pixel_offset + pixel_data as u32).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&pixel_offset.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&2i32.to_le_bytes());
    b.extend_from_slice(&2i32.to_le_bytes());
    b.extend_from_slice(&planes.to_le_bytes());
    b.extend_from_slice(&bits_per_pixel.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(pixel_data as u32).to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend(std::iter::repeat_n(0x42, pixel_data));
    b
}

fn png_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(chunk_type);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&[0, 0, 0, 0]); // CRC not validated by the sniffer
    chunk
}

fn apng_fixture() -> Vec<u8> {
    let mut b = PNG_SIGNATURE.to_vec();
    b.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
    b.extend(png_chunk(b"acTL", &[0, 0, 0, 2, 0, 0, 0, 0]));
    b.extend(png_chunk(b"IDAT", &[0x01, 0x02, 0x03, 0x04]));
    b.extend(png_chunk(b"IEND", &[]));
    b
}

#[test]
fn sniff_detects_supported_formats() {
    assert_eq!(detect_supported_image_mime(&solid_png(2, 2)), Some(SniffedMime::Png));
    assert_eq!(
        detect_supported_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, 74, 70]),
        Some(SniffedMime::Jpeg)
    );
    assert_eq!(detect_supported_image_mime(&gif_fixture()), Some(SniffedMime::Gif));
    let mut webp = b"RIFF\x24\x00\x00\x00WEBP".to_vec();
    webp.extend_from_slice(b"VP8 \x10\x00\x00\x00");
    assert_eq!(detect_supported_image_mime(&webp), Some(SniffedMime::WebP));
    assert_eq!(detect_supported_image_mime(&bmp_with(1, 24)), Some(SniffedMime::Bmp));
}

#[test]
fn sniff_rejects_non_images() {
    assert_eq!(detect_supported_image_mime(b""), None);
    assert_eq!(detect_supported_image_mime(&[0xFF, 0xD8]), None);
    assert_eq!(detect_supported_image_mime(b"# just a shell script\n"), None);
    assert_eq!(
        detect_supported_image_mime(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0]),
        None
    );
}

#[test]
fn sniff_rejects_apng_and_lossless_jpeg() {
    assert_eq!(detect_supported_image_mime(&apng_fixture()), None);
    assert_eq!(
        detect_supported_image_mime(&[0xFF, 0xD8, 0xFF, 0xF7, 0, 16, 0, 16]),
        None
    );
}

#[test]
fn apng_scan_stops_at_first_idat() {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
    bytes.extend(png_chunk(b"IDAT", &[1, 2, 3, 4]));
    bytes.extend(png_chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]));
    assert_eq!(detect_supported_image_mime(&bytes), Some(SniffedMime::Png));
}

#[test]
fn sniff_validates_bmp_headers() {
    assert_eq!(detect_supported_image_mime(&bmp_with(1, 24)), Some(SniffedMime::Bmp));
    assert_eq!(detect_supported_image_mime(&bmp_with(2, 24)), None); // planes must be 1
    assert_eq!(detect_supported_image_mime(&bmp_with(1, 3)), None); // unsupported depth
    let mut b = bmp_with(1, 24);
    b[14..18].copy_from_slice(&200u32.to_le_bytes());
    assert_eq!(detect_supported_image_mime(&b), None); // DIB size out of range
    b = bmp_with(1, 24);
    b[2..6].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(detect_supported_image_mime(&b), None); // declared size < 26
    b = bmp_with(1, 24);
    b[10..14].copy_from_slice(&20u32.to_le_bytes());
    assert_eq!(detect_supported_image_mime(&b), None); // pixel data before headers
    let truncated = bmp_with(1, 24)[..28].to_vec();
    assert_eq!(detect_supported_image_mime(&truncated), None); // truncated DIB
}

#[test]
fn small_png_passes_through_unchanged() {
    let bytes = solid_png(4, 4);
    let processed = process_image(&bytes, "image/png").unwrap();
    assert_eq!(processed.mime, "image/png");
    assert_eq!(processed.data, STANDARD.encode(&bytes));
    assert!(processed.hints.is_empty());
}

#[test]
fn gif_passes_through_without_reencoding() {
    let bytes = gif_fixture();
    let processed = process_image(&bytes, "image/gif").unwrap();
    assert_eq!(processed.mime, "image/gif");
    assert_eq!(processed.data, STANDARD.encode(&bytes));
    assert!(processed.hints.is_empty());
}

/// AC-003 (function level): oversized dimensions force the ladder; the emitted
/// image fits 2000×2000 and the note carries the original size and scale factor.
#[test]
fn oversized_dimensions_are_resized_with_dimension_note() {
    let bytes = solid_png(2100, 800);
    let processed = process_image(&bytes, "image/png").unwrap();
    assert_eq!(
        processed.hints,
        vec![
            "[Image: original 2100x800, displayed at 2000x762. Multiply coordinates by 1.05 to map to original image.]"
        ]
    );
    assert!(processed.data.len() < MAX_BASE64_BYTES);
    let raw = STANDARD.decode(&processed.data).unwrap();
    let img = image::load_from_memory(&raw).unwrap();
    assert_eq!((img.width(), img.height()), (2000, 762));
}

#[test]
fn slightly_oversized_png_downscales_to_dimension_limit() {
    let bytes = solid_png(2100, 100);
    let processed = process_image(&bytes, "image/png").unwrap();
    assert_eq!(processed.mime, "image/png");
    assert_eq!(
        processed.hints,
        vec![
            "[Image: original 2100x100, displayed at 2000x95. Multiply coordinates by 1.05 to map to original image.]"
        ]
    );
}

#[test]
fn bmp_is_converted_to_png_with_conversion_hint() {
    let bytes = bmp_with(1, 24);
    let processed = process_image(&bytes, "image/bmp").unwrap();
    assert_eq!(processed.mime, "image/png");
    assert_eq!(processed.hints, vec!["[Image converted from image/bmp to image/png.]"]);
    let raw = STANDARD.decode(&processed.data).unwrap();
    let img = image::load_from_memory(&raw).unwrap();
    assert_eq!((img.width(), img.height()), (2, 2));
    assert_ne!(processed.data, STANDARD.encode(&bytes));
}

#[test]
fn ladder_scaling_math_matches_pi() {
    let limits = ResizeLimits::INLINE;
    assert_eq!(limits.fit_dimensions(5120, 2880), (2000, 1125));
    assert_eq!(limits.fit_dimensions(2100, 100), (2000, 95));
    assert_eq!(limits.fit_dimensions(20000, 1), (2000, 1));
    assert_eq!(limits.fit_dimensions(1, 20000), (1, 2000));
    assert_eq!(shrink(2000), 1500);
    assert_eq!(shrink(2), 1);
    assert_eq!(shrink(1), 1);
}

#[test]
fn encoded_budget_forces_the_shrink_ladder() {
    let bytes = noise_png(64, 64);
    let limits = ResizeLimits {
        max_width: 2000,
        max_height: 2000,
        max_bytes: 2048,
    };
    let resized = resize_with_limits(&bytes, "image/png", limits).unwrap();
    assert!(resized.data.len() < 2048);
    assert!(dimension_note(&resized).is_some());
}

#[test]
fn ladder_fails_when_one_by_one_cannot_fit() {
    let bytes = solid_png(1, 1);
    let limits = ResizeLimits {
        max_width: 2000,
        max_height: 2000,
        max_bytes: 32,
    };
    assert!(resize_with_limits(&bytes, "image/png", limits).is_none());
}

#[test]
fn corrupt_image_reports_resize_failure() {
    let mut bytes = PNG_SIGNATURE.to_vec();
    bytes.extend(png_chunk(b"IHDR", &[0, 0, 0, 64, 0, 0, 0, 64, 8, 6, 0, 0, 0]));
    bytes.extend(png_chunk(b"IDAT", &[0x01, 0x02, 0x03, 0x04]));
    assert_eq!(detect_supported_image_mime(&bytes), Some(SniffedMime::Png));
    assert_eq!(process_image(&bytes, "image/png"), Err(ProcessError::Resize));
    assert_eq!(
        ProcessError::Resize.message(),
        "[Image omitted: could not be resized below the inline image size limit.]"
    );
}

#[test]
fn corrupt_bmp_reports_conversion_failure() {
    let mut bytes = bmp_with(1, 24);
    bytes.truncate(54); // headers intact, pixel data gone
    assert_eq!(detect_supported_image_mime(&bytes), Some(SniffedMime::Bmp));
    assert_eq!(process_image(&bytes, "image/bmp"), Err(ProcessError::Convert));
    assert_eq!(
        ProcessError::Convert.message(),
        "[Image omitted: could not be converted to a supported inline image format.]"
    );
}
