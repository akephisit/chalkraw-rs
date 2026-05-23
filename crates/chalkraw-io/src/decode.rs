use crate::error::IoError;
use chalkraw_core::ImageFormat;
use image::ImageReader;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Decoded source in linear sRGB, RGBA, 32-bit float per channel.
///
/// Pixels are stored row-major, four floats per pixel (R, G, B, A) in 0..1.
/// Phase 1 decodes JPEG/PNG/TIFF only; RAW arrives in Phase 4.
#[derive(Debug, Clone)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub pixels: Vec<f32>, // length = width * height * 4
}

impl LinearImage {
    pub fn stride_bytes(&self) -> usize {
        self.width as usize * 4 * std::mem::size_of::<f32>()
    }
}

fn match_format(detected: Option<image::ImageFormat>, ctx: &Path) -> Result<ImageFormat, IoError> {
    match detected {
        Some(image::ImageFormat::Jpeg) => Ok(ImageFormat::Jpeg),
        Some(image::ImageFormat::Png) => Ok(ImageFormat::Png),
        Some(image::ImageFormat::Tiff) => Ok(ImageFormat::Tiff),
        _ => Err(IoError::UnsupportedFormat(ctx.to_path_buf())),
    }
}

// sRGB 8-bit → linear f32 0..1 via IEC 61966-2-1 piecewise transfer (threshold 0.04045).
fn to_linear(rgba8: &image::RgbaImage) -> Vec<f32> {
    let (w, h) = rgba8.dimensions();
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for &c in rgba8.as_raw() {
        let v = c as f32 / 255.0;
        let linear = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        pixels.push(linear);
    }
    pixels
}

pub fn decode_image(path: impl AsRef<Path>) -> Result<LinearImage, IoError> {
    let path: PathBuf = path.as_ref().to_path_buf();

    let reader = ImageReader::open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
            _ => IoError::Io(e),
        })?
        .with_guessed_format()
        .map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e.into() })?;

    let format = match_format(reader.format(), &path)?;
    let dyn_img = reader.decode().map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e })?;
    let rgba8 = dyn_img.to_rgba8();
    let (w, h) = rgba8.dimensions();

    Ok(LinearImage { width: w, height: h, format, pixels: to_linear(&rgba8) })
}

/// Decode an in-memory image. Used as a fallback when the catalog's fixture
/// file is missing (e.g., a freshly downloaded binary launched from a folder
/// that doesn't contain `tests/fixtures/sample.jpg`).
pub fn decode_image_bytes(bytes: &[u8]) -> Result<LinearImage, IoError> {
    let synthetic = PathBuf::from("<embedded>");

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| IoError::DecodeFailed { path: synthetic.clone(), source: e.into() })?;

    let format = match_format(reader.format(), &synthetic)?;
    let dyn_img = reader.decode().map_err(|e| IoError::DecodeFailed { path: synthetic.clone(), source: e })?;
    let rgba8 = dyn_img.to_rgba8();
    let (w, h) = rgba8.dimensions();

    Ok(LinearImage { width: w, height: h, format, pixels: to_linear(&rgba8) })
}

/// Encode a 256-px-long-edge JPEG thumbnail from a decoded source.
/// Returns the JPEG byte stream for storage in `Photo.thumbnail`.
pub fn make_thumbnail(linear: &LinearImage) -> Result<Vec<u8>, IoError> {
    use image::{ImageBuffer, Rgba};
    let max_dim = 256u32;
    let (orig_w, orig_h) = (linear.width, linear.height);
    let scale = (max_dim as f32) / (orig_w.max(orig_h) as f32);
    let new_w = ((orig_w as f32) * scale).max(1.0) as u32;
    let new_h = ((orig_h as f32) * scale).max(1.0) as u32;

    // Convert linear f32 RGBA back to sRGB u8 for JPEG encoding.
    let mut src_bytes = Vec::with_capacity((orig_w * orig_h * 4) as usize);
    for &v in &linear.pixels {
        let v = v.clamp(0.0, 1.0);
        let srgb = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        src_bytes.push((srgb * 255.0).round() as u8);
    }
    let buffer: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(orig_w, orig_h, src_bytes)
        .ok_or_else(|| IoError::DecodeFailed {
            path: std::path::PathBuf::from("<thumbnail-encode>"),
            source: image::ImageError::Limits(image::error::LimitError::from_kind(
                image::error::LimitErrorKind::DimensionError,
            )),
        })?;

    let resized = image::imageops::resize(
        &buffer,
        new_w,
        new_h,
        image::imageops::FilterType::Triangle,
    );

    let mut out = Vec::new();
    let rgb: ImageBuffer<image::Rgb<u8>, _> = ImageBuffer::from_fn(new_w, new_h, |x, y| {
        let p = resized.get_pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    {
        use image::ImageEncoder;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
        encoder
            .write_image(
                rgb.as_raw(),
                new_w,
                new_h,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| IoError::DecodeFailed {
                path: std::path::PathBuf::from("<thumbnail-encode>"),
                source: e,
            })?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixture_jpeg() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/sample.jpg");
        let img = decode_image(path).expect("decode failed");
        assert_eq!(img.width, 1024);
        assert_eq!(img.height, 768);
        assert_eq!(img.format, ImageFormat::Jpeg);
        assert_eq!(img.pixels.len(), 1024 * 768 * 4);
        // Alpha channel should all be 1.0 (linear of 255)
        for px in img.pixels.chunks_exact(4) {
            assert!((px[3] - 1.0).abs() < 1e-6, "alpha must be 1.0, got {}", px[3]);
        }
    }

    #[test]
    fn missing_file_returns_not_found() {
        let err = decode_image("/no/such/path.jpg").unwrap_err();
        assert!(matches!(err, IoError::NotFound(_)));
    }

    #[test]
    fn decodes_png_to_linear() {
        let tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .unwrap();
        let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(4, 4, image::Rgba([128, 64, 32, 255]));
        buf.save(tmp.path()).unwrap();
        let img = decode_image(tmp.path()).expect("png decode");
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        assert_eq!(img.format, ImageFormat::Png);
        assert_eq!(img.pixels.len(), 4 * 4 * 4);
    }

    #[test]
    fn decodes_tiff_to_linear() {
        let tmp = tempfile::Builder::new()
            .suffix(".tiff")
            .tempfile()
            .unwrap();
        let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(2, 3, image::Rgba([10, 20, 30, 255]));
        buf.save(tmp.path()).unwrap();
        let img = decode_image(tmp.path()).expect("tiff decode");
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 3);
        assert_eq!(img.format, ImageFormat::Tiff);
        assert_eq!(img.pixels.len(), 2 * 3 * 4);
    }

    #[test]
    fn decodes_bytes_from_memory() {
        let mut buf = Vec::new();
        image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(4, 4, image::Rgba([255, 128, 64, 255]))
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let img = decode_image_bytes(&buf).expect("bytes decode");
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        assert_eq!(img.format, ImageFormat::Png);
        assert_eq!(img.pixels.len(), 4 * 4 * 4);
    }

    #[test]
    fn make_thumbnail_produces_jpeg_under_512_long_edge() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/sample.jpg");
        let img = decode_image(path).unwrap();
        let bytes = make_thumbnail(&img).unwrap();
        assert!(bytes.len() > 100, "thumbnail too small ({} bytes)", bytes.len());
        // Decode the thumbnail back and check dimensions.
        let decoded = image::load_from_memory(&bytes).unwrap();
        let (w, h) = (decoded.width(), decoded.height());
        assert!(w <= 256 && h <= 256);
        assert!(w == 256 || h == 256); // one edge must hit the max
    }
}
