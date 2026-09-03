use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, RgbImage, RgbaImage,
};

use super::sniff::image_format;

/// Maximum inline image dimensions (pi parity).
pub(crate) const MAX_DIMENSION: u32 = 2000;
/// 4.5MB of base64 payload — headroom below Anthropic's 5MB limit (pi parity).
pub(crate) const MAX_BASE64_BYTES: usize = 4_718_592;
/// JPEG quality ladder tried in order alongside PNG (pi parity).
const JPEG_QUALITIES: [u8; 5] = [80, 85, 70, 55, 40];

pub(crate) struct ResizedImage {
    pub data: String,
    pub mime: &'static str,
    pub original_width: u32,
    pub original_height: u32,
    pub width: u32,
    pub height: u32,
    pub was_resized: bool,
}

/// Inline-image limits (pi's `ImageResizeOptions` with its defaults).
pub(crate) struct ResizeLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_bytes: usize,
}

impl ResizeLimits {
    /// pi's defaults: 2000×2000 dimensions, 4.5MB base64 budget.
    pub const INLINE: Self = Self {
        max_width: MAX_DIMENSION,
        max_height: MAX_DIMENSION,
        max_bytes: MAX_BASE64_BYTES,
    };

    /// Scale to fit the limits, rounding like pi's `Math.round`. Unlike pi
    /// (which would throw on a zero-sized target), clamps to 1 so degenerate
    /// aspect ratios still produce a usable image.
    pub(crate) fn fit_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        let (mut w, mut h) = (width, height);
        if w > self.max_width {
            h = (f64::from(h) * f64::from(self.max_width) / f64::from(w))
                .round()
                .max(1.0) as u32;
            w = self.max_width;
        }
        if h > self.max_height {
            w = (f64::from(w) * f64::from(self.max_height) / f64::from(h))
                .round()
                .max(1.0) as u32;
            h = self.max_height;
        }
        (w, h)
    }
}

pub(crate) fn resize_to_limits(bytes: &[u8], mime: &'static str) -> Option<ResizedImage> {
    resize_with_limits(bytes, mime, ResizeLimits::INLINE)
}

/// Port of pi's `resizeImageInProcess`. Passes through when dimensions and the
/// base64 size already fit; otherwise decodes, applies EXIF orientation, then
/// tries PNG + JPEG candidates at decreasing sizes until one fits the budget.
pub(crate) fn resize_with_limits(bytes: &[u8], mime: &'static str, limits: ResizeLimits) -> Option<ResizedImage> {
    // Approximate base64 length without encoding (pi parity).
    let input_base64_size = bytes.len().div_ceil(3) * 4;
    let format = image_format(mime)?;
    let image = decode_with_orientation(bytes, format)?;
    let (original_width, original_height) = (image.width(), image.height());
    if original_width <= limits.max_width
        && original_height <= limits.max_height
        && input_base64_size < limits.max_bytes
    {
        return Some(ResizedImage {
            data: STANDARD.encode(bytes),
            mime,
            original_width,
            original_height,
            width: original_width,
            height: original_height,
            was_resized: false,
        });
    }

    let rgba = image.to_rgba8();
    let (mut width, mut height) = limits.fit_dimensions(original_width, original_height);
    loop {
        for (candidate_mime, data) in encode_candidates(&rgba, width, height) {
            if data.len() < limits.max_bytes {
                return Some(ResizedImage {
                    data,
                    mime: candidate_mime,
                    original_width,
                    original_height,
                    width,
                    height,
                    was_resized: true,
                });
            }
        }
        if (width, height) == (1, 1) {
            return None;
        }
        let next = (shrink(width), shrink(height));
        if next == (width, height) {
            return None;
        }
        (width, height) = next;
    }
}

/// `Math.max(1, Math.floor(v * 0.75))`, with 1 kept at 1 (pi parity).
pub(crate) fn shrink(v: u32) -> u32 {
    if v == 1 { 1 } else { (v * 3 / 4).max(1) }
}

/// PNG first, then JPEG across the quality ladder — first candidate under the
/// budget wins (pi parity). The resize re-samples from the original each round.
fn encode_candidates(rgba: &RgbaImage, width: u32, height: u32) -> Vec<(&'static str, String)> {
    let resized = image::imageops::resize(rgba, width, height, FilterType::Lanczos3);
    let mut candidates = Vec::new();
    if let Some(png) = encode_png(&resized) {
        candidates.push(("image/png", png));
    }
    let rgb = DynamicImage::ImageRgba8(resized).to_rgb8();
    for quality in JPEG_QUALITIES {
        if let Some(jpeg) = encode_jpeg(&rgb, quality) {
            candidates.push(("image/jpeg", jpeg));
        }
    }
    candidates
}

fn encode_png(rgba: &RgbaImage) -> Option<String> {
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(rgba.as_raw(), rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
        .ok()?;
    Some(STANDARD.encode(buf))
}

fn encode_jpeg(rgb: &RgbImage, quality: u8) -> Option<String> {
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
        .ok()?;
    Some(STANDARD.encode(buf))
}

/// Decode and apply EXIF orientation so rotated JPEGs display upright.
/// WebP EXIF is not exposed by `image-webp`; JPEG and PNG are covered.
fn decode_with_orientation(bytes: &[u8], format: ImageFormat) -> Option<DynamicImage> {
    let mut decoder = ImageReader::with_format(std::io::Cursor::new(bytes), format)
        .into_decoder()
        .ok()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    Some(image)
}
