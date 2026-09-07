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

fn detect_jpeg(bytes: &[u8]) -> Option<SniffedMime> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) && bytes.get(3) != Some(&0xF7) {
        Some(SniffedMime::Jpeg)
    } else {
        None
    }
}

fn detect_png(bytes: &[u8]) -> Option<SniffedMime> {
    if bytes.starts_with(&PNG_SIGNATURE) && is_png(bytes) && !is_animated_png(bytes) {
        Some(SniffedMime::Png)
    } else {
        None
    }
}

pub fn detect_supported_image_mime(bytes: &[u8]) -> Option<SniffedMime> {
    if let Some(jpeg) = detect_jpeg(bytes) {
        return Some(jpeg);
    }
    if let Some(png) = detect_png(bytes) {
        return Some(png);
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

fn validate_bmp_offsets(size: u32, offset: u32, dib_size: u32) -> bool {
    if size != 0 && (size < 26 || offset >= size) {
        return false;
    }
    u64::from(offset) >= 14 + u64::from(dib_size)
}

fn read_bmp_dimensions(bytes: &[u8], dib_size: u32) -> Option<(u16, u16)> {
    if dib_size == 12 {
        Some((read_u16_le(bytes, 22), read_u16_le(bytes, 24)))
    } else if (40..=124).contains(&dib_size) && bytes.len() >= 30 {
        Some((read_u16_le(bytes, 26), read_u16_le(bytes, 28)))
    } else {
        None
    }
}

fn is_bmp(bytes: &[u8]) -> bool {
    if bytes.len() < 26 {
        return false;
    }
    let dib_size = read_u32_le(bytes, 14);
    if !validate_bmp_offsets(read_u32_le(bytes, 2), read_u32_le(bytes, 10), dib_size) {
        return false;
    }
    let Some((planes, bpp)) = read_bmp_dimensions(bytes, dib_size) else {
        return false;
    };
    planes == 1 && matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32)
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
