//! Text rasterisation using ab_glyph.
//!
//! The embedded font is DejaVu Sans Regular, sourced from the Ubuntu system
//! package `fonts-dejavu-core` (SIL Open Font License 1.1 — compatible with
//! this project's MIT/Apache-2.0 dual licence).
//!
//! Only a single embedded font is supported in Phase 5B. Multiple fonts and
//! system-font browsing are deferred to a later phase.

use ab_glyph::{Font, FontRef, Glyph, PxScale, ScaleFont};
use image::{ImageBuffer, Rgba, RgbaImage};

/// Embedded font bytes: DejaVu Sans Regular (SIL OFL 1.1).
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/Default-Regular.ttf");

/// Rasterise `text` at the given pixel height into a tight-fitting RGBA image.
///
/// - Anti-aliased glyph coverage is stored in the alpha channel.
/// - `color` is `[r, g, b, a]`; the per-glyph alpha is `coverage * color[3]`.
/// - Returns `None` if the font fails to parse, the text is empty, or the
///   computed canvas dimensions are zero.
pub fn rasterise_text(text: &str, px_size: f32, color: [u8; 4]) -> Option<RgbaImage> {
    if text.is_empty() {
        return None;
    }
    let font = FontRef::try_from_slice(FONT_BYTES).ok()?;
    let scale = PxScale::from(px_size.max(1.0));
    let scaled = font.as_scaled(scale);

    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let line_height = (ascent - descent).ceil() as u32;

    // Lay out glyphs left-to-right, accumulating horizontal advance + kerning.
    let mut x_cursor: f32 = 0.0;
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut last_id: Option<ab_glyph::GlyphId> = None;

    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(prev) = last_id {
            x_cursor += scaled.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(x_cursor, ascent));
        x_cursor += scaled.h_advance(id);
        glyphs.push(glyph);
        last_id = Some(id);
    }

    // Add a 2-px pad on each side so no glyph clips at the edge.
    let width = (x_cursor.ceil() as u32).saturating_add(4);
    let height = line_height.saturating_add(4);
    if width == 0 || height == 0 {
        return None;
    }

    let mut img: RgbaImage = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));

    for glyph in glyphs {
        if let Some(outline) = font.outline_glyph(glyph) {
            let bb = outline.px_bounds();
            outline.draw(|x, y, coverage| {
                let px = bb.min.x as i32 + x as i32 + 2;
                let py = bb.min.y as i32 + y as i32 + 2;
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    return;
                }
                let a = (coverage * color[3] as f32) as u8;
                if a == 0 {
                    return;
                }
                img.put_pixel(
                    px as u32,
                    py as u32,
                    Rgba([color[0], color[1], color[2], a]),
                );
            });
        }
    }

    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterise_text_produces_nonempty_image() {
        let img = rasterise_text("HELLO", 32.0, [255, 255, 255, 255]);
        assert!(img.is_some());
        let img = img.unwrap();
        let (w, h) = img.dimensions();
        assert!(w > 0 && h > 0);
        // At least some pixels should have non-zero alpha.
        let mut nonzero = 0u32;
        for px in img.pixels() {
            if px[3] > 0 {
                nonzero += 1;
            }
        }
        assert!(nonzero > 0, "rasterised text should have visible pixels");
    }

    #[test]
    fn rasterise_empty_text_returns_none() {
        assert!(rasterise_text("", 32.0, [255, 255, 255, 255]).is_none());
    }
}
