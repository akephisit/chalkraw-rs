use crate::error::IoError;
use chalkraw_core::ImageFormat;
use image::ImageReader;
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

pub fn decode_image(path: impl AsRef<Path>) -> Result<LinearImage, IoError> {
    let path: PathBuf = path.as_ref().to_path_buf();

    let reader = ImageReader::open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
            _ => IoError::Io(e),
        })?
        .with_guessed_format()
        .map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e.into() })?;

    let format = match reader.format() {
        Some(image::ImageFormat::Jpeg) => ImageFormat::Jpeg,
        Some(image::ImageFormat::Png) => ImageFormat::Png,
        Some(image::ImageFormat::Tiff) => ImageFormat::Tiff,
        _ => return Err(IoError::UnsupportedFormat(path)),
    };

    let dyn_img = reader.decode().map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e })?;
    let rgba8 = dyn_img.to_rgba8();
    let (w, h) = rgba8.dimensions();

    // sRGB 8-bit → linear f32 0..1 via IEC 61966-2-1 piecewise transfer (threshold 0.04045).
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

    Ok(LinearImage { width: w, height: h, format, pixels })
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
}
