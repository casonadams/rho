use image::ImageFormat;

/// Magic-byte sniffing ported from pi's `detectSupportedImageMimeType`
/// (`packages/coding-agent/src/utils/mime.ts`). Only these five formats are
/// treated as images; everything else keeps rho's existing
/// `[Binary file: N bytes]` path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniffedMime {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
}

impl SniffedMime {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
            Self::Bmp => "image/bmp",
        }
    }
}

/// Read at most this many bytes before committing to sniffing (pi parity).
pub const SNIFF_WINDOW_BYTES: usize = 4100;

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

pub fn detect_supported_image_mime(bytes: &[u8]) -> Option<SniffedMime> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        // Lossless JPEG (FF D8 FF F7) is excluded (pi parity).
        return if bytes.get(3) == Some(&0xF7) {
            None
        } else {
            Some(SniffedMime::Jpeg)
        };
    }
    if bytes.starts_with(&PNG_SIGNATURE) {
        return if is_png(bytes) && !is_animated_png(bytes) {
            Some(SniffedMime::Png)
        } else {
            None
        };
    }
    if starts_with_ascii(bytes, 0, b"GIF") {
        return Some(SniffedMime::Gif);
    }
    if starts_with_ascii(bytes, 0, b"RIFF") && starts_with_ascii(bytes, 8, b"WEBP") {
        return Some(SniffedMime::WebP);
    }
    if starts_with_ascii(bytes, 0, b"BM") && is_bmp(bytes) {
        return Some(SniffedMime::Bmp);
    }
    None
}

/// PNG requires the signature to be followed by a 13-byte IHDR chunk header.
fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 16 && read_u32_be(bytes, 8) == 13 && starts_with_ascii(bytes, 12, b"IHDR")
}

/// Scan chunks for an `acTL` animation-control chunk; stop at the first
/// `IDAT` (static images put `acTL` before image data, if at all).
fn is_animated_png(bytes: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while offset + 8 <= bytes.len() {
        let chunk_length = read_u32_be(bytes, offset) as usize;
        let chunk_type_offset = offset + 4;
        if starts_with_ascii(bytes, chunk_type_offset, b"acTL") {
            return true;
        }
        if starts_with_ascii(bytes, chunk_type_offset, b"IDAT") {
            return false;
        }
        let next = offset + 8 + chunk_length + 4;
        if next <= offset || next > bytes.len() {
            return false;
        }
        offset = next;
    }
    false
}

fn is_bmp(bytes: &[u8]) -> bool {
    if bytes.len() < 26 {
        return false;
    }
    let declared_file_size = read_u32_le(bytes, 2);
    let pixel_data_offset = read_u32_le(bytes, 10);
    let dib_header_size = read_u32_le(bytes, 14);
    if declared_file_size != 0 && declared_file_size < 26 {
        return false;
    }
    if u64::from(pixel_data_offset) < 14 + u64::from(dib_header_size) {
        return false;
    }
    if declared_file_size != 0 && pixel_data_offset >= declared_file_size {
        return false;
    }
    let (color_planes, bits_per_pixel) = if dib_header_size == 12 {
        (read_u16_le(bytes, 22), read_u16_le(bytes, 24))
    } else if (40..=124).contains(&dib_header_size) {
        if bytes.len() < 30 {
            return false;
        }
        (read_u16_le(bytes, 26), read_u16_le(bytes, 28))
    } else {
        return false;
    };
    color_planes == 1 && matches!(bits_per_pixel, 1 | 4 | 8 | 16 | 24 | 32)
}

fn read_u32_be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn read_u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn starts_with_ascii(bytes: &[u8], offset: usize, marker: &[u8]) -> bool {
    bytes.len() >= offset + marker.len() && &bytes[offset..offset + marker.len()] == marker
}

pub(crate) fn image_format(mime: &str) -> Option<ImageFormat> {
    match mime {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}
