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

// ── RAW format helpers ─────────────────────────────────────────────────────────

/// Return a `RawFormat` if `path`'s extension matches a known RAW extension.
/// Case-insensitive (e.g. `.CR2`, `.nef`, `.Arw` all work).
pub(crate) fn raw_format_from_extension(path: &Path) -> Option<chalkraw_core::RawFormat> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "cr2" => Some(chalkraw_core::RawFormat::CanonCr2),
        "cr3" => Some(chalkraw_core::RawFormat::CanonCr3),
        "nef" => Some(chalkraw_core::RawFormat::NikonNef),
        "arw" => Some(chalkraw_core::RawFormat::SonyArw),
        "raf" => Some(chalkraw_core::RawFormat::FujiRaf),
        "pef" => Some(chalkraw_core::RawFormat::PentaxPef),
        "orf" => Some(chalkraw_core::RawFormat::OlympusOrf),
        _ => None,
    }
}

/// Scan `bytes` for embedded JPEG streams (SOI = 0xFFD8 0xFF, EOI = 0xFFD9).
/// Returns the *largest* JPEG found, which is almost always the full-sized
/// camera preview rather than a small thumbnail.
pub(crate) fn find_embedded_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            // Possible SOI — scan forward for the matching EOI marker.
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == 0xFF && bytes[j + 1] == 0xD9 {
                    candidates.push((i, j + 2));
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    // Pick the largest candidate.
    candidates
        .into_iter()
        .max_by_key(|(start, end)| end - start)
        .map(|(start, end)| bytes[start..end].to_vec())
}

/// Decode a RAW file via rawloader.
///
/// Strategy (v1): extract the camera's embedded JPEG preview, which most
/// modern cameras include at full resolution as a fast-load convenience.
/// This is an 8-bit sRGB image with the camera's own tone/colour rendering
/// baked in — it is NOT a high-bit-depth linear decode, but it is
/// immediately viewable and editable.
///
/// If no usable JPEG preview is found, falls back to a half-resolution
/// greyscale demosaic using 2×2 Bayer cell averaging.  This is intentionally
/// crude; it ensures the function always returns *something* rather than
/// returning an error on cameras whose RAW containers carry no preview.
fn decode_raw(path: PathBuf, raw_format: chalkraw_core::RawFormat) -> Result<LinearImage, IoError> {
    // Open the file once to give rawloader a chance to validate it, and
    // again to read the raw bytes for JPEG scanning.
    {
        let file = std::fs::File::open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
            _ => IoError::Io(e),
        })?;
        let mut reader = std::io::BufReader::new(file);
        // We attempt to decode to confirm rawloader recognises the format.
        // If it fails we still attempt the JPEG-scan fallback below.
        let _ = rawloader::decode(&mut reader);
    }

    // Read the full file into memory for JPEG scanning.
    let bytes = std::fs::read(&path)?;

    if let Some(jpeg_bytes) = find_embedded_jpeg(&bytes) {
        // Decode the embedded JPEG preview.
        match decode_image_bytes(&jpeg_bytes) {
            Ok(linear) => {
                return Ok(LinearImage {
                    format: chalkraw_core::ImageFormat::Raw(raw_format),
                    ..linear
                });
            }
            Err(e) => {
                log::warn!("embedded JPEG decode failed for {path:?}: {e}; trying rawloader demosaic");
            }
        }
    }

    // No usable JPEG preview — fall back to a half-resolution demosaic via
    // rawloader's Bayer data.
    let file2 = std::fs::File::open(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
        _ => IoError::Io(e),
    })?;
    let mut reader2 = std::io::BufReader::new(file2);
    let raw = rawloader::decode(&mut reader2).map_err(|_| IoError::DecodeFailed {
        path: path.clone(),
        source: image::ImageError::Limits(image::error::LimitError::from_kind(
            image::error::LimitErrorKind::DimensionError,
        )),
    })?;

    simple_half_res_demosaic(&raw, raw_format, &path)
}

/// Half-resolution greyscale demosaic: average each 2×2 Bayer cell into one
/// pixel.  Not colour-correct, but produces a usable image that at least
/// shows the scene content when no JPEG preview is available.
fn simple_half_res_demosaic(
    raw: &rawloader::RawImage,
    raw_format: chalkraw_core::RawFormat,
    path: &Path,
) -> Result<LinearImage, IoError> {
    let w = raw.width;
    let h = raw.height;
    if w < 2 || h < 2 {
        return Err(IoError::DecodeFailed {
            path: path.to_path_buf(),
            source: image::ImageError::Limits(image::error::LimitError::from_kind(
                image::error::LimitErrorKind::DimensionError,
            )),
        });
    }
    let half_w = (w / 2) as u32;
    let half_h = (h / 2) as u32;
    let mut pixels = Vec::with_capacity((half_w * half_h * 4) as usize);

    match &raw.data {
        rawloader::RawImageData::Integer(data) => {
            let max = raw.whitelevels.iter().copied().max().unwrap_or(16383) as f32;
            for y in 0..half_h as usize {
                for x in 0..half_w as usize {
                    let sx = x * 2;
                    let sy = y * 2;
                    let i0 = sy * w + sx;
                    let i1 = i0 + 1;
                    let i2 = i0 + w;
                    let i3 = i2 + 1;
                    let avg = (data[i0] as f32
                        + data[i1] as f32
                        + data[i2] as f32
                        + data[i3] as f32)
                        / 4.0;
                    let n = (avg / max).clamp(0.0, 1.0);
                    pixels.push(n);
                    pixels.push(n);
                    pixels.push(n);
                    pixels.push(1.0);
                }
            }
        }
        rawloader::RawImageData::Float(data) => {
            for y in 0..half_h as usize {
                for x in 0..half_w as usize {
                    let sx = x * 2;
                    let sy = y * 2;
                    let i0 = sy * w + sx;
                    let i1 = i0 + 1;
                    let i2 = i0 + w;
                    let i3 = i2 + 1;
                    let avg = (data[i0] + data[i1] + data[i2] + data[i3]) / 4.0;
                    let n = avg.clamp(0.0, 1.0);
                    pixels.push(n);
                    pixels.push(n);
                    pixels.push(n);
                    pixels.push(1.0);
                }
            }
        }
    }

    Ok(LinearImage {
        width: half_w,
        height: half_h,
        format: chalkraw_core::ImageFormat::Raw(raw_format),
        pixels,
    })
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

/// Read the EXIF Orientation tag (1–8) from a file.
/// Returns 1 (identity) when the file has no EXIF or no Orientation tag.
fn read_exif_orientation(path: &Path) -> u32 {
    use exif::{In, Reader, Tag};
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return 1,
    };
    let mut buf = std::io::BufReader::new(file);
    let exif = match Reader::new().read_from_container(&mut buf) {
        Ok(e) => e,
        Err(_) => return 1,
    };
    exif.get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1)
}

/// Read the EXIF Orientation tag (1–8) from an in-memory byte slice.
/// Returns 1 (identity) when there is no EXIF or no Orientation tag.
fn read_exif_orientation_bytes(bytes: &[u8]) -> u32 {
    use exif::{In, Reader, Tag};
    let mut cur = Cursor::new(bytes);
    let exif = match Reader::new().read_from_container(&mut cur) {
        Ok(e) => e,
        Err(_) => return 1,
    };
    exif.get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1)
}

/// Apply the EXIF orientation transform to an RgbaImage.
/// Orientation values follow the EXIF spec (JEITA CP-3451C Table 1).
pub(crate) fn apply_orientation(img: image::RgbaImage, orientation: u32) -> image::RgbaImage {
    use image::imageops;
    match orientation {
        2 => imageops::flip_horizontal(&img),
        3 => imageops::rotate180(&img),
        4 => imageops::flip_vertical(&img),
        5 => imageops::rotate90(&imageops::flip_horizontal(&img)),
        6 => imageops::rotate90(&img),
        7 => imageops::rotate270(&imageops::flip_horizontal(&img)),
        8 => imageops::rotate270(&img),
        _ => img, // orientation 1 or unknown → no-op
    }
}

/// Log the presence of an embedded ICC profile in a JPEG file.
/// Full ICC colour management is deferred to a dedicated phase; this log
/// helps diagnose "colours look different from source" reports.
fn log_icc_profile_if_present(path: &Path) {
    if let Ok(file) = std::fs::File::open(path) {
        let reader = std::io::BufReader::new(file);
        if let Ok(mut decoder) = image::codecs::jpeg::JpegDecoder::new(reader) {
            use image::ImageDecoder;
            if let Ok(Some(icc)) = decoder.icc_profile() {
                log::info!(
                    "decode_image {path:?}: ICC profile present ({} bytes), treating as sRGB — \
                     full ICC colour management deferred to a future phase",
                    icc.len()
                );
            }
        }
    }
}

pub fn decode_image(path: impl AsRef<Path>) -> Result<LinearImage, IoError> {
    let path: PathBuf = path.as_ref().to_path_buf();

    // Phase 4: route known RAW extensions to the RAW decode path before
    // attempting the standard image crate reader (which does not support RAW).
    if let Some(raw_format) = raw_format_from_extension(&path) {
        return decode_raw(path, raw_format);
    }

    // Issue 3: log ICC profile presence so users can confirm the cause of
    // colour differences when their camera embeds a non-sRGB profile.
    log_icc_profile_if_present(&path);

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

    // Issue 1: apply EXIF orientation before converting to linear.
    let orientation = read_exif_orientation(&path);
    let rgba8 = apply_orientation(rgba8, orientation);

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

    // Issue 1: apply EXIF orientation from the byte stream.
    let orientation = read_exif_orientation_bytes(bytes);
    let rgba8 = apply_orientation(rgba8, orientation);

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
    fn orientation_1_is_identity() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/sample.jpg");
        let img = decode_image(path).unwrap();
        // sample.jpg is 1024×768 with no EXIF orientation — dimensions must be unchanged.
        assert_eq!(img.width, 1024);
        assert_eq!(img.height, 768);
    }

    #[test]
    fn apply_orientation_rotates_90_swaps_dimensions() {
        let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_fn(10, 20, |x, _| {
            image::Rgba([x as u8, 0, 0, 255])
        });
        let rotated = apply_orientation(img, 6); // 90 CW swaps width and height
        assert_eq!(rotated.dimensions(), (20, 10));
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

    #[test]
    fn raw_format_from_extension_recognises_common_raws() {
        assert_eq!(
            raw_format_from_extension(Path::new("foo.cr2")),
            Some(chalkraw_core::RawFormat::CanonCr2)
        );
        assert_eq!(
            raw_format_from_extension(Path::new("foo.CR3")),
            Some(chalkraw_core::RawFormat::CanonCr3)
        );
        assert_eq!(
            raw_format_from_extension(Path::new("foo.NEF")),
            Some(chalkraw_core::RawFormat::NikonNef)
        );
        assert_eq!(
            raw_format_from_extension(Path::new("foo.arw")),
            Some(chalkraw_core::RawFormat::SonyArw)
        );
        assert_eq!(raw_format_from_extension(Path::new("foo.jpg")), None);
        assert_eq!(raw_format_from_extension(Path::new("foo")), None);
    }

    #[test]
    fn find_embedded_jpeg_finds_marker_run() {
        // Build a minimal SOI APP0 ... EOI sequence at offset 2.
        let mut bytes = vec![0x00u8, 0x01];
        bytes.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
        bytes.extend_from_slice(&[0x12, 0x34, 0x56]);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes.extend_from_slice(&[0x00, 0x00]);
        let jpeg = find_embedded_jpeg(&bytes);
        assert!(jpeg.is_some(), "expected to find a JPEG");
        let jpeg = jpeg.unwrap();
        assert!(
            jpeg.starts_with(&[0xFF, 0xD8]),
            "should start with SOI"
        );
        assert!(
            jpeg.ends_with(&[0xFF, 0xD9]),
            "should end with EOI"
        );
    }
}
