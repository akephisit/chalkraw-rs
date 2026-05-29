use crate::error::IoError;
use chalkraw_core::ImageFormat;
use image::ImageReader;
use rayon::prelude::*;
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
/// Priority order (Phase 4 v3 — AHD):
/// 1. Full-resolution AHD-style directional Bayer demosaic (real RAW decode).
/// 2. Embedded JPEG preview (fast, camera-processed, 8-bit fallback).
/// 3. Half-resolution greyscale average (last resort).
fn decode_raw(path: PathBuf, raw_format: chalkraw_core::RawFormat) -> Result<LinearImage, IoError> {
    // Decode the RAW file with rawloader.
    let raw = {
        let file = std::fs::File::open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
            _ => IoError::Io(e),
        })?;
        let mut reader = std::io::BufReader::new(file);
        rawloader::decode(&mut reader).ok()
    };

    // ── 1. Try AHD-style directional demosaic ─────────────────────────────
    if let Some(ref raw_img) = raw {
        if let Some(rgb_f32) = demosaic_ahd(raw_img) {
            log::info!(
                "decode_raw {path:?}: AHD demosaic {}x{}",
                rgb_f32.width(),
                rgb_f32.height()
            );
            return Ok(rgb_f32_to_linear_image(
                rgb_f32,
                chalkraw_core::ImageFormat::Raw(raw_format),
            ));
        }
    }

    log::info!("decode_raw {path:?}: demosaic unavailable, trying embedded JPEG preview");

    // ── 2. Embedded JPEG preview fallback ─────────────────────────────────
    let bytes = std::fs::read(&path)?;
    if let Some(jpeg_bytes) = find_embedded_jpeg(&bytes) {
        match decode_image_bytes(&jpeg_bytes) {
            Ok(linear) => {
                log::info!("decode_raw {path:?}: using embedded JPEG preview");
                return Ok(LinearImage {
                    format: chalkraw_core::ImageFormat::Raw(raw_format),
                    ..linear
                });
            }
            Err(e) => {
                log::warn!("decode_raw {path:?}: embedded JPEG decode failed: {e}");
            }
        }
    }

    // ── 3. Half-resolution greyscale demosaic (last resort) ───────────────
    let raw_img = match raw {
        Some(r) => r,
        None => {
            // rawloader failed earlier; try once more now (file may have been
            // inaccessible the first time if it was a transient error).
            let file = std::fs::File::open(&path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
                _ => IoError::Io(e),
            })?;
            let mut reader = std::io::BufReader::new(file);
            rawloader::decode(&mut reader).map_err(|_| IoError::DecodeFailed {
                path: path.clone(),
                source: image::ImageError::Limits(image::error::LimitError::from_kind(
                    image::error::LimitErrorKind::DimensionError,
                )),
            })?
        }
    };

    simple_half_res_demosaic(&raw_img, raw_format, &path)
}

/// Full-resolution bilinear Bayer demosaic.
///
/// Retained as a reference implementation and potential fallback.
/// The active demosaic path is [`demosaic_ahd`].
///
/// For each output pixel the known channel comes directly from the sensor;
/// the two missing channels are interpolated by averaging all neighbours of
/// that colour within the 3×3 window centred on the pixel.
///
/// Black-level subtraction and white-level normalisation are applied so the
/// output is in the 0..1 linear-light range (sensor spectral sensitivities,
/// not yet converted to a display colour space).
///
/// Camera colour-matrix transformation is applied via `cam_to_xyz_normalized`
/// plus the standard XYZ-D65 → sRGB matrix, converting sensor values to
/// linear sRGB. Clamping to 0..1 is applied after the matrix multiply.
///
/// Returns `None` if the image is monochrome, too small, or uses an
/// unsupported (non-Bayer) CFA pattern.
#[allow(dead_code)]
fn demosaic_bilinear(
    raw: &rawloader::RawImage,
) -> Option<image::ImageBuffer<image::Rgb<f32>, Vec<f32>>> {
    use rawloader::RawImageData;

    // Only handle single-component (Bayer) images.
    if raw.cpp != 1 {
        return None;
    }
    let cfa = raw.cropped_cfa();
    if !cfa.is_valid() {
        return None; // monochrome sensor
    }

    let w = raw.width;
    let h = raw.height;
    if w < 2 || h < 2 {
        return None;
    }

    // Per-channel black level and white level for normalisation.
    let blacks = raw.blacklevels;
    let whites = raw.whitelevels;

    // Normalise a raw integer sample to 0..1, with per-channel black subtraction.
    let normalise_int = |v: u16, ch: usize| -> f32 {
        let black = blacks[ch] as f32;
        let white = whites[ch] as f32;
        let range = (white - black).max(1.0);
        ((v as f32 - black) / range).clamp(0.0, 1.0)
    };

    // Build the normalised sensor plane.
    let normalised: Vec<f32> = match &raw.data {
        RawImageData::Integer(d) => d
            .iter()
            .enumerate()
            .map(|(idx, &v)| {
                // Determine which CFA colour this pixel maps to.
                let row = idx / w;
                let col = idx % w;
                let ch = cfa.color_at(row, col);
                normalise_int(v, ch)
            })
            .collect(),
        RawImageData::Float(d) => d.iter().map(|&v| v.clamp(0.0, 1.0)).collect(),
    };

    // ── Bilinear demosaic ─────────────────────────────────────────────────
    let mut out_r = vec![0.0f32; w * h];
    let mut out_g = vec![0.0f32; w * h];
    let mut out_b = vec![0.0f32; w * h];

    for y in 0..h {
        for x in 0..w {
            let center_ch = cfa.color_at(y, x);
            let center_v = normalised[y * w + x];

            let mut sum = [0.0f32; 3];
            let mut cnt = [0u32; 3];

            // Accumulate all 3×3 neighbours (including center).
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let nch = cfa.color_at(ny as usize, nx as usize);
                    let nv = normalised[ny as usize * w + nx as usize];
                    sum[nch] += nv;
                    cnt[nch] += 1;
                }
            }

            // The center pixel contributes its own measured value exactly.
            // For the other two channels use the neighbourhood average.
            let idx = y * w + x;
            out_r[idx] = if center_ch == 0 {
                center_v
            } else if cnt[0] > 0 {
                sum[0] / cnt[0] as f32
            } else {
                0.0
            };
            out_g[idx] = if center_ch == 1 {
                center_v
            } else if cnt[1] > 0 {
                sum[1] / cnt[1] as f32
            } else {
                0.0
            };
            out_b[idx] = if center_ch == 2 {
                center_v
            } else if cnt[2] > 0 {
                sum[2] / cnt[2] as f32
            } else {
                0.0
            };
        }
    }

    // ── Camera colour-matrix application ──────────────────────────────────
    // `cam_to_xyz_normalized` returns a 3×4 matrix [out_xyz][in_cam_rgbe].
    // We only use the first 3 camera channels (R, G, B) and ignore E.
    //
    // XYZ (D65) → linear sRGB matrix (IEC 61966-2-1):
    //   [ 3.2406, -1.5372, -0.4986 ]
    //   [-0.9689,  1.8758,  0.0415 ]
    //   [ 0.0557, -0.2040,  1.0570 ]
    let cam_to_xyz = raw.cam_to_xyz_normalized();
    // Combine: sRGB = XYZ_to_sRGB * cam_to_xyz * cam
    // XYZ_to_sRGB rows:
    let xyz_to_srgb: [[f32; 3]; 3] = [
        [3.2406, -1.5372, -0.4986],
        [-0.9689, 1.8758, 0.0415],
        [0.0557, -0.2040, 1.0570],
    ];

    // Pre-multiply: srgb_from_cam[srgb_ch][cam_ch] (cam_ch in 0..3 only)
    let mut m = [[0.0f32; 3]; 3];
    for s in 0..3 {
        for c in 0..3 {
            for xyz_ch in 0..3 {
                // cam_to_xyz[xyz_ch][c] — column c of row xyz_ch
                m[s][c] += xyz_to_srgb[s][xyz_ch] * cam_to_xyz[xyz_ch][c];
            }
        }
    }

    // Apply the combined matrix per pixel.
    let mut buf = image::ImageBuffer::<image::Rgb<f32>, Vec<f32>>::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let cr = out_r[idx];
            let cg = out_g[idx];
            let cb = out_b[idx];
            let sr = (m[0][0] * cr + m[0][1] * cg + m[0][2] * cb).clamp(0.0, 1.0);
            let sg = (m[1][0] * cr + m[1][1] * cg + m[1][2] * cb).clamp(0.0, 1.0);
            let sb = (m[2][0] * cr + m[2][1] * cg + m[2][2] * cb).clamp(0.0, 1.0);
            buf.put_pixel(x as u32, y as u32, image::Rgb([sr, sg, sb]));
        }
    }

    Some(buf)
}

/// Multiply two 3×3 f32 matrices: out = a * b.
fn matmul_3x3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            for k in 0..3 {
                out[r][c] += a[r][k] * b[k][c];
            }
        }
    }
    out
}

/// AHD-style (Adaptive Homogeneity-Directed) demosaic — simplified directional variant.
///
/// Stage 1 – Green reconstruction at R/B sites:
///   Two candidate G values are computed (horizontal average and vertical average)
///   each corrected with a 2nd-order curvature term (the "AHD correction").
///   The direction with the smaller gradient wins.  Edge pixels (within 2 px of the
///   border) fall back to plain 4-neighbour bilinear.
///
/// Stage 2 – R and B reconstruction:
///   Missing R (or B) values are estimated by averaging the colour difference
///   (R − G) from the nearest R (or B) neighbours and adding it back to the
///   already-known G at each site.  This is smoother than interpolating R/B
///   directly because the difference channel has far less high-frequency energy.
///
/// The camera-to-XYZ-D65 matrix from rawloader is combined with the standard
/// XYZ-D65 → linear-sRGB matrix and applied per pixel, identical to the
/// previous bilinear path.
///
/// Returns `None` for monochrome sensors, cpp != 1, images smaller than 4×4,
/// or invalid CFA patterns.
#[allow(clippy::needless_range_loop)] // `x` is used as a 2-D coordinate, not just an index
fn demosaic_ahd(
    raw: &rawloader::RawImage,
) -> Option<image::ImageBuffer<image::Rgb<f32>, Vec<f32>>> {
    let (w, h) = (raw.width, raw.height);
    if w < 4 || h < 4 || raw.cpp != 1 {
        return None;
    }
    let cfa = raw.cropped_cfa();
    if !cfa.is_valid() {
        return None;
    }

    // ── Normalise raw values per-channel using black/white levels ─────────
    let raw_normalised: Vec<f32> = match &raw.data {
        rawloader::RawImageData::Integer(d) => d
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let y = i / w;
                let x = i % w;
                let ch = cfa.color_at(y, x).min(3);
                let bl = raw.blacklevels[ch] as f32;
                let wl = (raw.whitelevels[ch] as f32 - bl).max(1.0);
                ((v as f32 - bl) / wl).clamp(0.0, 1.0)
            })
            .collect(),
        rawloader::RawImageData::Float(d) => d.iter().copied().map(|v| v.clamp(0.0, 1.0)).collect(),
    };

    let get = |y: usize, x: usize| -> f32 { raw_normalised[y * w + x] };

    // ── Stage 1: G channel reconstruction (parallel, row-by-row) ─────────
    let g_rows: Vec<Vec<f32>> = (0..h)
        .into_par_iter()
        .map(|y| {
            let mut row = vec![0.0f32; w];
            for x in 0..w {
                let cc = cfa.color_at(y, x);
                let val = if cc == 1 {
                    // Already a green site.
                    get(y, x)
                } else if y >= 2 && y < h - 2 && x >= 2 && x < w - 2 {
                    // R or B site — directional interpolation with curvature correction.
                    let c = get(y, x);
                    // Horizontal candidate.
                    let gh = (get(y, x - 1) + get(y, x + 1)) * 0.5
                        + (2.0 * c - get(y, x - 2) - get(y, x + 2)) * 0.25;
                    // Vertical candidate.
                    let gv = (get(y - 1, x) + get(y + 1, x)) * 0.5
                        + (2.0 * c - get(y - 2, x) - get(y + 2, x)) * 0.25;
                    // Gradient magnitudes.
                    let grad_h = (get(y, x - 1) - get(y, x + 1)).abs()
                        + (2.0 * c - get(y, x - 2) - get(y, x + 2)).abs();
                    let grad_v = (get(y - 1, x) - get(y + 1, x)).abs()
                        + (2.0 * c - get(y - 2, x) - get(y + 2, x)).abs();
                    if grad_h <= grad_v {
                        gh
                    } else {
                        gv
                    }
                } else {
                    // Border pixel: plain 4-neighbour bilinear fallback.
                    let mut sum = 0.0f32;
                    let mut count = 0u32;
                    for (dy, dx) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let ny = y as i32 + dy;
                        let nx = x as i32 + dx;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        if cfa.color_at(ny as usize, nx as usize) == 1 {
                            sum += get(ny as usize, nx as usize);
                            count += 1;
                        }
                    }
                    if count > 0 {
                        sum / count as f32
                    } else {
                        get(y, x)
                    }
                };
                row[x] = val.clamp(0.0, 1.0);
            }
            row
        })
        .collect();

    // Flatten row vecs into a contiguous plane.
    let g_plane: Vec<f32> = g_rows.into_iter().flatten().collect();

    // ── Stage 2: R and B reconstruction (parallel, row-by-row) ───────────
    // Use colour-difference interpolation: average (channel − G) from the
    // nearest same-colour neighbours, then add back the G at the current site.
    let rb_rows: Vec<(Vec<f32>, Vec<f32>)> = (0..h)
        .into_par_iter()
        .map(|y| {
            let mut r_row = vec![0.0f32; w];
            let mut b_row = vec![0.0f32; w];
            for x in 0..w {
                let cc = cfa.color_at(y, x);
                let g_here = g_plane[y * w + x];

                // Known R site: store directly.
                if cc == 0 {
                    r_row[x] = get(y, x);
                } else {
                    // Missing R: average (R − G) from R neighbours.
                    let mut sum_diff = 0.0f32;
                    let mut count = 0u32;
                    for (dy, dx) in [
                        (-1i32, -1i32),
                        (-1, 1),
                        (1, -1),
                        (1, 1),
                        (-1, 0),
                        (1, 0),
                        (0, -1),
                        (0, 1),
                    ] {
                        let ny = y as i32 + dy;
                        let nx = x as i32 + dx;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let nyu = ny as usize;
                        let nxu = nx as usize;
                        if cfa.color_at(nyu, nxu) == 0 {
                            sum_diff += get(nyu, nxu) - g_plane[nyu * w + nxu];
                            count += 1;
                        }
                    }
                    r_row[x] = if count > 0 {
                        (g_here + sum_diff / count as f32).clamp(0.0, 1.0)
                    } else {
                        g_here
                    };
                }

                // Known B site: store directly.
                if cc == 2 {
                    b_row[x] = get(y, x);
                } else {
                    // Missing B: average (B − G) from B neighbours.
                    let mut sum_diff = 0.0f32;
                    let mut count = 0u32;
                    for (dy, dx) in [
                        (-1i32, -1i32),
                        (-1, 1),
                        (1, -1),
                        (1, 1),
                        (-1, 0),
                        (1, 0),
                        (0, -1),
                        (0, 1),
                    ] {
                        let ny = y as i32 + dy;
                        let nx = x as i32 + dx;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let nyu = ny as usize;
                        let nxu = nx as usize;
                        if cfa.color_at(nyu, nxu) == 2 {
                            sum_diff += get(nyu, nxu) - g_plane[nyu * w + nxu];
                            count += 1;
                        }
                    }
                    b_row[x] = if count > 0 {
                        (g_here + sum_diff / count as f32).clamp(0.0, 1.0)
                    } else {
                        g_here
                    };
                }
            }
            (r_row, b_row)
        })
        .collect();

    let (r_plane, b_plane): (Vec<f32>, Vec<f32>) = {
        let mut r = Vec::with_capacity(w * h);
        let mut b = Vec::with_capacity(w * h);
        for (rr, br) in rb_rows {
            r.extend_from_slice(&rr);
            b.extend_from_slice(&br);
        }
        (r, b)
    };

    // ── Apply camera colour matrix ─────────────────────────────────────────
    // cam_to_xyz_normalized() → 3×4; we use only the first 3 cam channels.
    let cam_to_xyz_raw = raw.cam_to_xyz_normalized();
    let cam_to_xyz: [[f32; 3]; 3] = [
        [
            cam_to_xyz_raw[0][0],
            cam_to_xyz_raw[0][1],
            cam_to_xyz_raw[0][2],
        ],
        [
            cam_to_xyz_raw[1][0],
            cam_to_xyz_raw[1][1],
            cam_to_xyz_raw[1][2],
        ],
        [
            cam_to_xyz_raw[2][0],
            cam_to_xyz_raw[2][1],
            cam_to_xyz_raw[2][2],
        ],
    ];
    let xyz_to_srgb: [[f32; 3]; 3] = [
        [3.240_454, -1.537_138_5, -0.498_531_4],
        [-0.969_266, 1.876_010_8, 0.041_556],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];
    let m = matmul_3x3(&xyz_to_srgb, &cam_to_xyz);

    // ── Assemble output image ──────────────────────────────────────────────
    let mut out = image::ImageBuffer::<image::Rgb<f32>, _>::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let r_cam = r_plane[i];
            let g_cam = g_plane[i];
            let b_cam = b_plane[i];
            let r = (m[0][0] * r_cam + m[0][1] * g_cam + m[0][2] * b_cam).clamp(0.0, 1.0);
            let g = (m[1][0] * r_cam + m[1][1] * g_cam + m[1][2] * b_cam).clamp(0.0, 1.0);
            let b = (m[2][0] * r_cam + m[2][1] * g_cam + m[2][2] * b_cam).clamp(0.0, 1.0);
            out.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
        }
    }

    Some(out)
}

/// Convert an Rgb<f32> image buffer into a `LinearImage` (RGBA f32, alpha=1).
fn rgb_f32_to_linear_image(
    img: image::ImageBuffer<image::Rgb<f32>, Vec<f32>>,
    format: chalkraw_core::ImageFormat,
) -> LinearImage {
    let (w, h) = img.dimensions();
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for p in img.pixels() {
        pixels.push(p[0].clamp(0.0, 1.0));
        pixels.push(p[1].clamp(0.0, 1.0));
        pixels.push(p[2].clamp(0.0, 1.0));
        pixels.push(1.0);
    }
    LinearImage {
        width: w,
        height: h,
        format,
        pixels,
    }
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
                    let avg =
                        (data[i0] as f32 + data[i1] as f32 + data[i2] as f32 + data[i3] as f32)
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

/// Read the embedded ICC profile bytes from an in-memory JPEG, PNG, or TIFF.
/// Returns `None` if no profile is present or the bytes cannot be decoded by
/// the relevant container decoder.
fn read_icc_profile_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;

    if let Ok(mut decoder) = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)) {
        if let Ok(Some(icc)) = decoder.icc_profile() {
            return Some(icc);
        }
    }

    if let Ok(mut decoder) = image::codecs::png::PngDecoder::new(Cursor::new(bytes)) {
        if let Ok(Some(icc)) = decoder.icc_profile() {
            return Some(icc);
        }
    }

    if let Ok(mut decoder) = image::codecs::tiff::TiffDecoder::new(Cursor::new(bytes)) {
        if let Ok(Some(icc)) = decoder.icc_profile() {
            return Some(icc);
        }
    }

    None
}

fn convert_icc_bytes_to_srgb(ctx: &Path, bytes: &[u8], rgba: &mut image::RgbaImage) {
    let icc_bytes = match read_icc_profile_bytes(bytes) {
        Some(b) => b,
        None => return,
    };

    let src_profile = match qcms::Profile::new_from_slice(&icc_bytes, true) {
        Some(p) => p,
        None => {
            log::warn!(
                "decode_image {ctx:?}: embedded ICC profile could not be parsed, treating as sRGB"
            );
            return;
        }
    };

    if src_profile.is_sRGB() {
        log::debug!("decode_image {ctx:?}: embedded ICC profile is sRGB — no transform needed");
        return;
    }

    let dst_profile = qcms::Profile::new_sRGB();
    let transform = match qcms::Transform::new_to(
        &src_profile,
        &dst_profile,
        qcms::DataType::RGBA8,
        qcms::DataType::RGBA8,
        qcms::Intent::Perceptual,
    ) {
        Some(t) => t,
        None => {
            log::warn!(
                "decode_image {ctx:?}: ICC→sRGB transform creation failed, treating as sRGB"
            );
            return;
        }
    };

    log::info!(
        "decode_image {ctx:?}: applying embedded ICC profile ({} bytes) → sRGB transform",
        icc_bytes.len()
    );
    transform.apply(rgba.as_mut());
}

/// Decode already-read image bytes using `ctx` only for extension-based RAW
/// routing and error messages. For RAW files, this still delegates to the
/// path-based RAW loader because rawloader needs a reader over the original file.
pub fn decode_image_from_bytes(
    ctx: impl AsRef<Path>,
    bytes: &[u8],
) -> Result<LinearImage, IoError> {
    let ctx = ctx.as_ref().to_path_buf();
    if let Some(raw_format) = raw_format_from_extension(&ctx) {
        return decode_raw(ctx, raw_format);
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| IoError::DecodeFailed {
            path: ctx.clone(),
            source: e.into(),
        })?;
    let format = match_format(reader.format(), &ctx)?;
    let dyn_img = reader.decode().map_err(|e| IoError::DecodeFailed {
        path: ctx.clone(),
        source: e,
    })?;
    let rgba8 = dyn_img.to_rgba8();
    let orientation = read_exif_orientation_bytes(bytes);
    let mut rgba8 = apply_orientation(rgba8, orientation);
    convert_icc_bytes_to_srgb(&ctx, bytes, &mut rgba8);
    let (w, h) = rgba8.dimensions();
    Ok(LinearImage {
        width: w,
        height: h,
        format,
        pixels: to_linear(&rgba8),
    })
}

pub fn decode_image(path: impl AsRef<Path>) -> Result<LinearImage, IoError> {
    let path: PathBuf = path.as_ref().to_path_buf();

    // Phase 4: route known RAW extensions to the RAW decode path before
    // attempting the standard image crate reader (which does not support RAW).
    if let Some(raw_format) = raw_format_from_extension(&path) {
        return decode_raw(path, raw_format);
    }

    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IoError::NotFound(path.clone()),
        _ => IoError::Io(e),
    })?;
    decode_image_from_bytes(&path, &bytes)
}

/// Decode an in-memory image. Used as a fallback when the catalog's fixture
/// file is missing (e.g., a freshly downloaded binary launched from a folder
/// that doesn't contain `tests/fixtures/sample.jpg`).
pub fn decode_image_bytes(bytes: &[u8]) -> Result<LinearImage, IoError> {
    let synthetic = PathBuf::from("<embedded>");
    decode_image_from_bytes(synthetic, bytes)
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

    let resized =
        image::imageops::resize(&buffer, new_w, new_h, image::imageops::FilterType::Triangle);

    let mut out = Vec::new();
    let rgb: ImageBuffer<image::Rgb<u8>, _> = ImageBuffer::from_fn(new_w, new_h, |x, y| {
        let p = resized.get_pixel(x, y);
        image::Rgb([p[0], p[1], p[2]])
    });
    {
        use image::ImageEncoder;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
        encoder
            .write_image(rgb.as_raw(), new_w, new_h, image::ExtendedColorType::Rgb8)
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
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sample.jpg"
        );
        let img = decode_image(path).expect("decode failed");
        assert_eq!(img.width, 1024);
        assert_eq!(img.height, 768);
        assert_eq!(img.format, ImageFormat::Jpeg);
        assert_eq!(img.pixels.len(), 1024 * 768 * 4);
        // Alpha channel should all be 1.0 (linear of 255)
        for px in img.pixels.chunks_exact(4) {
            assert!(
                (px[3] - 1.0).abs() < 1e-6,
                "alpha must be 1.0, got {}",
                px[3]
            );
        }
    }

    #[test]
    fn missing_file_returns_not_found() {
        let err = decode_image("/no/such/path.jpg").unwrap_err();
        assert!(matches!(err, IoError::NotFound(_)));
    }

    #[test]
    fn decodes_png_to_linear() {
        let tmp = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            4,
            4,
            image::Rgba([128, 64, 32, 255]),
        );
        buf.save(tmp.path()).unwrap();
        let img = decode_image(tmp.path()).expect("png decode");
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 4);
        assert_eq!(img.format, ImageFormat::Png);
        assert_eq!(img.pixels.len(), 4 * 4 * 4);
    }

    #[test]
    fn decodes_tiff_to_linear() {
        let tmp = tempfile::Builder::new().suffix(".tiff").tempfile().unwrap();
        let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            2,
            3,
            image::Rgba([10, 20, 30, 255]),
        );
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
        image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            4,
            4,
            image::Rgba([255, 128, 64, 255]),
        )
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
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sample.jpg"
        );
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
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sample.jpg"
        );
        let img = decode_image(path).unwrap();
        let bytes = make_thumbnail(&img).unwrap();
        assert!(
            bytes.len() > 100,
            "thumbnail too small ({} bytes)",
            bytes.len()
        );
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
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "should start with SOI");
        assert!(jpeg.ends_with(&[0xFF, 0xD9]), "should end with EOI");
    }

    /// Regression: ICC path must not break decode of a profile-less JPEG.
    #[test]
    fn decode_no_icc_jpeg_unchanged() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sample.jpg"
        );
        let img = decode_image(path).expect("decode failed");
        assert_eq!(img.width, 1024);
        assert_eq!(img.height, 768);
        // Alpha must be 1.0 throughout.
        for px in img.pixels.chunks_exact(4) {
            assert!(
                (px[3] - 1.0).abs() < 1e-6,
                "alpha must be 1.0, got {}",
                px[3]
            );
        }
    }

    /// An sRGB→sRGB qcms transform must leave pixel values unchanged.
    #[test]
    fn qcms_srgb_to_srgb_is_identity() {
        let src = qcms::Profile::new_sRGB();
        let dst = qcms::Profile::new_sRGB();
        let xfm = qcms::Transform::new_to(
            &src,
            &dst,
            qcms::DataType::RGBA8,
            qcms::DataType::RGBA8,
            qcms::Intent::Perceptual,
        )
        .expect("sRGB→sRGB transform must succeed");

        let original = vec![128u8, 64, 32, 255, 200, 100, 50, 255];
        let mut pixels = original.clone();
        xfm.apply(&mut pixels);
        // sRGB→sRGB with perceptual intent should round-trip cleanly.
        assert_eq!(pixels, original, "sRGB→sRGB transform must be a no-op");
    }
}
