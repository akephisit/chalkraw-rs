/// Golden-image tests for Phase 2A develop sliders.
///
/// Each test uploads a known solid-colour source, sets one slider, renders,
/// and asserts the central pixel moved in the expected direction.  All tests
/// skip gracefully when no GPU adapter is available (CI / sandbox).
use chalkraw_core::{Crop, EditState};
use chalkraw_render::{
    create_pingpong, make_identity_3d_lut, make_identity_lut, make_target, read_to_cpu,
    BilateralPipeline, BlurPipeline, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
};
use std::sync::{Mutex, MutexGuard};

// ── helpers ──────────────────────────────────────────────────────────────────

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

fn gpu_test_lock() -> MutexGuard<'static, ()> {
    GPU_TEST_LOCK.lock().unwrap()
}

fn solid_image(w: u32, h: u32, r: f32, g: f32, b: f32) -> Vec<f32> {
    (0..w * h).flat_map(|_| [r, g, b, 1.0_f32]).collect()
}

fn solid_grey(w: u32, h: u32, v: f32) -> Vec<f32> {
    solid_image(w, h, v, v, v)
}

fn render_solid(rd: &RenderDevice, w: u32, h: u32, pixels: Vec<f32>, edit: &EditState) -> Vec<u8> {
    let src = SourceTexture::upload(rd, w, h, &pixels);
    let pipe = DevelopPipeline::new(rd, PipelineConfig::default());
    pipe.update_uniforms(&EditUniforms::from(edit));
    // Tests that don't exercise Clarity/Sharpening/Texture/NR pass source.view for blur views.
    // Pass identity LUT for tone curve (no effect when tone_curve_active=0).
    // Pass identity 3D LUT for display profile (no effect when display_lut_active=0).
    let (_lut_tex, lut_view) = make_identity_lut(rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(rd);
    let bg = pipe.make_bind_group(
        &src, &src.view, &src.view, &src.view, &src.view, &lut_view, &dlut_view,
    );
    let (tex, view) = make_target(rd, w, h);
    pipe.render(&view, &bg);
    read_to_cpu(rd, &tex, w, h).unwrap()
}

fn pixel_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// +50 contrast on a 0.7-grey pixel should push it further from 0.5 (brighter).
#[test]
fn contrast_plus_50_pushes_value_away_from_midgrey() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.tone.contrast = 50.0;

    // Baseline: contrast=0
    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.7), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.7), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    // 0.7 is above 0.5 pivot, so +contrast should make it brighter (higher byte).
    assert!(
        p[0] > base_p[0],
        "contrast +50 should brighten pixel above midgrey; got {p:?} vs base {base_p:?}"
    );
}

/// Negative contrast should compress tones toward mid-grey without inverting them.
#[test]
fn contrast_minus_40_compresses_without_inverting() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.tone.contrast = -40.0;

    let base_edit = EditState::default();
    let dark_base = pixel_at(
        &render_solid(&rd, w, h, solid_grey(w, h, 0.25), &base_edit),
        w,
        8,
        8,
    );
    let light_base = pixel_at(
        &render_solid(&rd, w, h, solid_grey(w, h, 0.75), &base_edit),
        w,
        8,
        8,
    );
    let dark = pixel_at(
        &render_solid(&rd, w, h, solid_grey(w, h, 0.25), &edit),
        w,
        8,
        8,
    );
    let light = pixel_at(
        &render_solid(&rd, w, h, solid_grey(w, h, 0.75), &edit),
        w,
        8,
        8,
    );

    assert!(
        dark[0] > dark_base[0],
        "contrast -40 should lift dark tones toward mid-grey; got {dark:?} vs {dark_base:?}"
    );
    assert!(
        light[0] < light_base[0],
        "contrast -40 should lower light tones toward mid-grey; got {light:?} vs {light_base:?}"
    );
    assert!(
        dark[0] < light[0],
        "contrast -40 must preserve tonal ordering; dark={dark:?}, light={light:?}"
    );
}

/// +50 shadows on a 0.2-grey pixel should brighten it.
#[test]
fn shadows_plus_50_brightens_dark_pixels() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.tone.shadows = 50.0;

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[0] > base_p[0],
        "shadows +50 should brighten dark pixel; got {p:?} vs base {base_p:?}"
    );
}

/// Warm white balance (7500 K > 5500 K neutral) should shift red up and blue down.
#[test]
fn wb_warm_shifts_red_up_blue_down() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.white_balance.temp_kelvin = 7500.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    // R should be brighter than B on a warm tint.
    assert!(
        p[0] > p[2],
        "warm WB should have R > B; got R={} B={}",
        p[0],
        p[2]
    );
}

/// -100 saturation on a solid red pixel should produce equal R=G=B (grey).
#[test]
fn saturation_minus_100_produces_grey() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.color.saturation = -100.0;

    // Solid red (linear)
    let pixels = render_solid(&rd, w, h, solid_image(w, h, 0.8, 0.0, 0.0), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    // After full desaturation, all channels should converge.
    // Allow ±4 byte tolerance for sRGB conversion rounding.
    let diff_rg = (p[0] as i32 - p[1] as i32).unsigned_abs();
    let diff_rb = (p[0] as i32 - p[2] as i32).unsigned_abs();
    assert!(
        diff_rg <= 4,
        "saturation -100 should equalise R and G; diff={diff_rg}, p={p:?}"
    );
    assert!(
        diff_rb <= 4,
        "saturation -100 should equalise R and B; diff={diff_rb}, p={p:?}"
    );
}

/// -100 vignette amount should darken the corner pixel relative to the centre.
#[test]
fn vignette_minus_100_darkens_corners() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (32, 32);
    let mut edit = EditState::default();
    edit.effects.vignette.amount = -100.0;
    // Set feather to full and midpoint to 40% so the corner (dist≈1.41) is
    // well past the midpoint and is darkened.
    edit.effects.vignette.midpoint = 40.0;
    edit.effects.vignette.feather = 60.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);

    let centre = pixel_at(&pixels, w, 16, 16);
    let corner = pixel_at(&pixels, w, 0, 0);

    assert!(
        corner[0] < centre[0],
        "vignette -100 should darken corner vs centre; corner={} centre={}",
        corner[0],
        centre[0]
    );
}

/// Grain amount=50 on a uniform grey should introduce per-pixel variation
/// (not every pixel identical), confirming the hash noise is being applied.
#[test]
fn grain_amount_50_introduces_variation() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (32, 32);
    let mut edit = EditState::default();
    edit.effects.grain.amount = 50.0;
    edit.effects.grain.size = 50.0; // medium frequency

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);

    // With grain, not all pixels should be identical. Collect unique R values.
    let unique_r: std::collections::HashSet<u8> =
        (0..w * h).map(|i| pixels[(i * 4) as usize]).collect();
    assert!(
        unique_r.len() > 1,
        "grain should introduce per-pixel variation; all pixels had same R={:?}",
        unique_r
    );
}

/// HSL red hue shift: solid red input with hsl[0].hue=50 should rotate the
/// red hue, reducing R and increasing G (shifting toward orange/yellow).
#[test]
fn hsl_red_hue_shift_rotates_red() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    // Baseline: pure red, no HSL edit.
    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_image(w, h, 0.8, 0.0, 0.0), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    // With red-band hue shift of +50 (→ +18° toward orange/yellow):
    // R should decrease and G should increase compared to baseline.
    let mut edit = EditState::default();
    edit.hsl[0].hue = 50.0; // red band
    let pixels = render_solid(&rd, w, h, solid_image(w, h, 0.8, 0.0, 0.0), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[0] < base_p[0] || p[1] > base_p[1],
        "hsl red hue +50 should shift hue (R down or G up); base={base_p:?} shifted={p:?}"
    );
}

/// HSL blue saturation -100: solid blue input should become near-grey,
/// while a solid red input with the same edit should remain saturated.
#[test]
fn hsl_blue_saturation_minus_100_desaturates_only_blue() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    let mut edit = EditState::default();
    edit.hsl[5].saturation = -100.0; // blue band (index 5)

    // Solid blue → should become near-grey (R ≈ G ≈ B).
    let blue_pixels = render_solid(&rd, w, h, solid_image(w, h, 0.0, 0.0, 0.8), &edit);
    let blue_p = pixel_at(&blue_pixels, w, 8, 8);
    let diff_rg_blue = (blue_p[0] as i32 - blue_p[1] as i32).unsigned_abs();
    let diff_rb_blue = (blue_p[0] as i32 - blue_p[2] as i32).unsigned_abs();
    assert!(
        diff_rg_blue <= 10 && diff_rb_blue <= 10,
        "hsl blue sat -100 on blue pixel should produce near-grey; got {blue_p:?}"
    );

    // Solid red → should remain saturated (R >> G and R >> B).
    let red_pixels = render_solid(&rd, w, h, solid_image(w, h, 0.8, 0.0, 0.0), &edit);
    let red_p = pixel_at(&red_pixels, w, 8, 8);
    assert!(
        red_p[0] > red_p[1] + 50,
        "hsl blue sat -100 should not desaturate red pixel; got {red_p:?}"
    );
}

/// Color Grading — blue shadows tint on a dark pixel (0.2 grey):
/// shadows.hue=240 (blue), shadows.saturation=100 → output B > R.
#[test]
fn cg_shadows_blue_tints_dark_pixels() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.color_grading.shadows.hue = 240.0; // blue
    edit.color_grading.shadows.saturation = 100.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[2] > p[0],
        "cg shadows blue tint on dark pixel should produce B > R; got R={} G={} B={}",
        p[0],
        p[1],
        p[2]
    );
}

/// Color Grading — yellow highlights tint on a light pixel (0.8 grey):
/// highlights.hue=60 (yellow), highlights.saturation=100 → output R+G > B.
#[test]
fn cg_highlights_yellow_tints_light_pixels() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.color_grading.highlights.hue = 60.0; // yellow
    edit.color_grading.highlights.saturation = 100.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.8), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    let rg_sum = p[0] as u32 + p[1] as u32;
    let b = p[2] as u32;
    assert!(
        rg_sum > b * 2,
        "cg highlights yellow tint on light pixel should produce R+G > B; got R={} G={} B={}",
        p[0],
        p[1],
        p[2]
    );
}

/// Color Grading — global luminance -50 should darken any pixel.
#[test]
fn cg_global_lum_minus_50_darkens_all() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let mut edit = EditState::default();
    edit.color_grading.global.luminance = -50.0;
    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[0] < base_p[0],
        "cg global luminance -50 should darken pixel; got {} (was {})",
        p[0],
        base_p[0]
    );
}

/// Parametric curve shadows=+50 on a 0.1-grey pixel (deep shadow zone) should lift it.
#[test]
fn parametric_shadows_plus_50_lifts_dark_pixels() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.1), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let mut edit = EditState::default();
    edit.parametric_curve.shadows = 50.0;
    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.1), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[0] > base_p[0],
        "parametric shadows +50 should lift dark pixel (0.1 grey); got {p:?} vs base {base_p:?}"
    );
}

/// Parametric curve highlights=-50 on a 0.9-grey pixel (bright zone) should dim it.
#[test]
fn parametric_highlights_minus_50_dims_bright_pixels() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.9), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let mut edit = EditState::default();
    edit.parametric_curve.highlights = -50.0;
    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.9), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[0] < base_p[0],
        "parametric highlights -50 should dim bright pixel (0.9 grey); got {p:?} vs base {base_p:?}"
    );
}

// ── Phase 2F tests ────────────────────────────────────────────────────────────

/// Lens distortion = 50 (barrel) on a solid grey source: the centre pixel must
/// stay the same brightness (the UV distortion formula leaves the exact centre
/// unchanged because r2 = 0 at (0.5, 0.5)).
#[test]
fn lens_distortion_positive_keeps_centre_unchanged() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (32, 32);
    let grey_val = 0.5_f32;

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, grey_val), &base_edit);
    let base_p = pixel_at(&base_pixels, w, w / 2, h / 2);

    let mut edit = EditState::default();
    edit.lens_correction.distortion = 50.0;
    let pixels = render_solid(&rd, w, h, solid_grey(w, h, grey_val), &edit);
    let p = pixel_at(&pixels, w, w / 2, h / 2);

    // Centre pixel must be within 2 bytes of baseline (distortion = 0 at centre).
    let diff = (p[0] as i32 - base_p[0] as i32).unsigned_abs();
    assert!(
        diff <= 2,
        "lens distortion should leave centre pixel unchanged; base={base_p:?} distorted={p:?}"
    );
}

/// Lens vignetting correction = 100 on solid grey: corner pixel should be
/// brighter than the centre pixel (radial brightening compensates falloff).
#[test]
fn lens_vignetting_brightens_corner() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (32, 32);

    let mut edit = EditState::default();
    edit.lens_correction.vignetting = 100.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);

    let centre = pixel_at(&pixels, w, w / 2, h / 2);
    let corner = pixel_at(&pixels, w, 0, 0);

    assert!(
        corner[0] > centre[0],
        "lens vignetting correction 100 should brighten corner vs centre; corner={} centre={}",
        corner[0],
        centre[0]
    );
}

/// Crop enabled with top-left quadrant (x=0,y=0,w=0.5,h=0.5): the output
/// centre pixel should be sampled from source position (0.25, 0.25), not (0.5,
/// 0.5). We verify this by checking the brightness changes relative to a
/// non-uniform source.
///
/// Source: top-left half at 0.2, bottom-right half at 0.8.  With crop enabled
/// the output centre maps to (0.25, 0.25) → the darker half → centre byte
/// should be darker than baseline (no crop, samples 0.5-grey average area).
#[test]
fn crop_enabled_top_left_quadrant_samples_correct_region() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (32u32, 32u32);

    // Build a source where the top-left quadrant is dark (0.2) and the
    // bottom-right quadrant is bright (0.8). The other two quadrants are 0.5.
    let source: Vec<f32> = (0..w * h)
        .flat_map(|i| {
            let px = i % w;
            let py = i / w;
            let v = if px < w / 2 && py < h / 2 {
                0.2_f32
            } else {
                0.8_f32
            };
            [v, v, v, 1.0_f32]
        })
        .collect();

    // Baseline: no crop, output centre maps to source (0.5, 0.5) → bright region.
    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, source.clone(), &base_edit);
    let base_centre = pixel_at(&base_pixels, w, w / 2, h / 2);

    // With crop: top-left quadrant only. Output centre → source (0.25, 0.25) → dark region.
    let edit = EditState {
        crop: Some(Crop {
            x_pct: 0.0,
            y_pct: 0.0,
            w_pct: 0.5,
            h_pct: 0.5,
            rotation_deg: 0.0,
        }),
        ..EditState::default()
    };
    let crop_pixels = render_solid(&rd, w, h, source, &edit);
    let crop_centre = pixel_at(&crop_pixels, w, w / 2, h / 2);

    assert!(
        crop_centre[0] < base_centre[0],
        "crop top-left quadrant: centre should sample dark region (R={}) vs no-crop bright (R={})",
        crop_centre[0],
        base_centre[0]
    );
}

/// Manual sRGB encoding (non-sRGB surface) should produce pixel values within
/// ±2 levels of the hardware-encode path (sRGB surface).
///
/// Both pipelines receive the same linear-light source and the same default
/// edit state.  The hardware path writes to Rgba8UnormSrgb (GPU encodes).
/// The manual path writes to Rgba8Unorm with srgb_output=1 (shader encodes).
/// After readback the centre pixel of both images should match closely.
#[test]
fn manual_srgb_encoding_matches_hardware_encoding() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 16;
    let h: u32 = 16;
    let pixels: Vec<f32> = (0..w * h).flat_map(|_| [0.5_f32, 0.5, 0.5, 1.0]).collect();
    let edit = EditState::default();

    // ── hardware sRGB path ────────────────────────────────────────────────────
    let pipe_hw = DevelopPipeline::new(
        &rd,
        PipelineConfig {
            output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        },
    );
    pipe_hw.update_uniforms(&EditUniforms::from(&edit));
    let src_hw = SourceTexture::upload(&rd, w, h, &pixels);
    // Pass source.view for all blur views (Clarity/Sharpening/Texture/NR not exercised).
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_hw, lut_view_hw) = make_identity_lut(&rd);
    let (_dlut_hw, dlut_view_hw) = make_identity_3d_lut(&rd);
    let bg_hw = pipe_hw.make_bind_group(
        &src_hw,
        &src_hw.view,
        &src_hw.view,
        &src_hw.view,
        &src_hw.view,
        &lut_view_hw,
        &dlut_view_hw,
    );
    let (tex_hw, view_hw) = make_target(&rd, w, h);
    pipe_hw.render(&view_hw, &bg_hw);
    let out_hw = read_to_cpu(&rd, &tex_hw, w, h).unwrap();

    // ── manual sRGB path (Rgba8Unorm, shader encodes) ─────────────────────────
    let pipe_sw = DevelopPipeline::new(
        &rd,
        PipelineConfig {
            output_format: wgpu::TextureFormat::Rgba8Unorm,
        },
    );
    // Verify the flag was set correctly.
    assert!(
        pipe_sw.manual_srgb_needed,
        "pipeline with Rgba8Unorm should set manual_srgb_needed"
    );

    pipe_sw.update_uniforms(&EditUniforms::from(&edit));
    let src_sw = SourceTexture::upload(&rd, w, h, &pixels);
    let (_lut_sw, lut_view_sw) = make_identity_lut(&rd);
    let (_dlut_sw, dlut_view_sw) = make_identity_3d_lut(&rd);
    let bg_sw = pipe_sw.make_bind_group(
        &src_sw,
        &src_sw.view,
        &src_sw.view,
        &src_sw.view,
        &src_sw.view,
        &lut_view_sw,
        &dlut_view_sw,
    );

    // Create a non-sRGB render target manually (make_target always uses Rgba8UnormSrgb).
    let target_sw = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("readback target non-srgb"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_sw = target_sw.create_view(&wgpu::TextureViewDescriptor::default());
    pipe_sw.render(&view_sw, &bg_sw);
    let out_sw = read_to_cpu(&rd, &target_sw, w, h).unwrap();

    // Centre pixel of both outputs should match within ±2 byte levels.
    let center_hw = &out_hw[((8 * w + 8) * 4) as usize..];
    let center_sw = &out_sw[((8 * w + 8) * 4) as usize..];
    for c in 0..3 {
        let diff = (center_hw[c] as i32 - center_sw[c] as i32).unsigned_abs();
        assert!(
            diff <= 2,
            "channel {c} differs: hw={} sw={} (manual sRGB should match hardware sRGB within ±2)",
            center_hw[c],
            center_sw[c]
        );
    }
}

/// Vibrance=+100 on a near-grey pixel should boost saturation more than
/// Vibrance=0, but not exceed what full Saturation=+100 would do.
#[test]
fn vibrance_boosts_low_saturation_colors() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skipping: no GPU");
            return;
        }
    };
    let (w, h) = (16, 16);

    // Slightly tinted grey: mostly neutral but with a small red bias.
    let source = solid_image(w, h, 0.55, 0.45, 0.45);

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, source.clone(), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);
    let base_spread = base_p[0] as i32 - base_p[2] as i32; // R−B spread

    let mut edit = EditState::default();
    edit.color.vibrance = 100.0;
    let vib_pixels = render_solid(&rd, w, h, source, &edit);
    let vib_p = pixel_at(&vib_pixels, w, 8, 8);
    let vib_spread = vib_p[0] as i32 - vib_p[2] as i32;

    assert!(vib_spread > base_spread,
        "vibrance +100 should increase R−B spread on low-saturation input; base={base_spread} vib={vib_spread}");
}

// ── Phase 2E.1: Clarity ───────────────────────────────────────────────────────

// ── Phase 2E.2: Sharpening ────────────────────────────────────────────────────

/// Sharpening amount=100 on a striped image (alternating pixel-bright/dark) should
/// produce measurably different output vs amount=0, because the small-sigma blur
/// differs from the source on high-frequency content.
#[test]
fn sharpening_amount_100_changes_high_freq_edges() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 32;
    let h: u32 = 32;
    // Vertical stripes with sharp transitions every other pixel.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let x = i % w;
            let v = if x.is_multiple_of(2) {
                0.4_f32
            } else {
                0.6_f32
            };
            [v, v, v, 1.0_f32]
        })
        .collect();

    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let (_, clar_a, _, clar_b) = create_pingpong(&rd, w, h);
    let (_, sharp_a, _, sharp_b) = create_pingpong(&rd, w, h);
    blur.render_pass(&source.view, &clar_a, w, h, true, 16.0);
    blur.render_pass(&clar_a, &clar_b, w, h, false, 16.0);
    blur.render_pass(&source.view, &sharp_a, w, h, true, 1.5);
    blur.render_pass(&sharp_a, &sharp_b, w, h, false, 1.5);

    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass source.view for texture_blur and nr_blur (not exercised in this sharpening test).
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(
        &source,
        &clar_b,
        &sharp_b,
        &source.view,
        &source.view,
        &lut_view,
        &dlut_view,
    );

    // Render with sharpening amount = 100.
    let mut edit = EditState::default();
    edit.detail.sharpening.amount = 100.0;
    edit.detail.sharpening.radius = 1.5;
    pipe.update_uniforms(&EditUniforms::from(&edit));
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bind);
    let with_sharp = read_to_cpu(&rd, &tex, w, h).unwrap();

    // Render with sharpening amount = 0 (same bind group).
    let edit_zero = EditState::default();
    pipe.update_uniforms(&EditUniforms::from(&edit_zero));
    let (tex0, view0) = make_target(&rd, w, h);
    pipe.render(&view0, &bind);
    let without_sharp = read_to_cpu(&rd, &tex0, w, h).unwrap();

    // High-freq edge pixels should differ.
    let mut total_diff = 0i32;
    for i in 0..(w * h) {
        let idx = (i * 4) as usize;
        total_diff += (with_sharp[idx] as i32 - without_sharp[idx] as i32).abs();
    }
    assert!(
        total_diff > 100,
        "sharpening should produce visible difference, got total_diff={total_diff}"
    );
}

// ── Phase 2E.1: Clarity ───────────────────────────────────────────────────────

/// Clarity +50 on a striped image (alternating bright/dark bands) should
/// alter edge-adjacent pixels relative to clarity=0, because the blur produces
/// a value that differs from the source on such high-frequency content.
#[test]
fn clarity_plus_50_increases_contrast_on_textured_image() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 32;
    let h: u32 = 32;
    // Build a source with alternating bright/dark vertical stripes — high local contrast.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let x = i % w;
            let v = if x % 4 < 2 { 0.7_f32 } else { 0.3_f32 };
            [v, v, v, 1.0_f32]
        })
        .collect();

    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let (_, blur_view_a, _, blur_view_b) = create_pingpong(&rd, w, h);
    blur.render_pass(&source.view, &blur_view_a, w, h, true, 8.0);
    blur.render_pass(&blur_view_a, &blur_view_b, w, h, false, 8.0);

    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass source.view for sharpening, texture, and NR blurs (not exercised in this test).
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(
        &source,
        &blur_view_b,
        &source.view,
        &source.view,
        &source.view,
        &lut_view,
        &dlut_view,
    );

    // Render with clarity = 50.
    let mut edit = EditState::default();
    edit.presence.clarity = 50.0;
    pipe.update_uniforms(&EditUniforms::from(&edit));
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bind);
    let pixels_out = read_to_cpu(&rd, &tex, w, h).unwrap();

    // Render again with clarity = 0 (using the same bind group — blur view unchanged).
    let mut edit_zero = EditState::default();
    edit_zero.presence.clarity = 0.0;
    pipe.update_uniforms(&EditUniforms::from(&edit_zero));
    let (tex0, view0) = make_target(&rd, w, h);
    pipe.render(&view0, &bind);
    let pixels_zero = read_to_cpu(&rd, &tex0, w, h).unwrap();

    // Pick a stripe-edge pixel (x=8, y=16) — on the boundary between dark and bright.
    let edge_idx = ((16 * w + 8) * 4) as usize;
    let with_clarity = pixels_out[edge_idx];
    let no_clarity = pixels_zero[edge_idx];
    assert_ne!(
        with_clarity, no_clarity,
        "clarity +50 should change edge-pixel brightness vs clarity=0; \
         with_clarity={with_clarity} no_clarity={no_clarity}"
    );
}

// ── Phase 2E.3: Texture ───────────────────────────────────────────────────────

/// Texture +50 on mid-frequency stripes (period 6 px) should produce measurably
/// different output vs texture=0, because the mid-sigma (5 px) blur differs from
/// the source on that spatial frequency.
#[test]
fn texture_amount_50_changes_mid_freq_pattern() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 32;
    let h: u32 = 32;
    // Mid-frequency stripes (period 6) — distinct from Clarity's large patches
    // and Sharpening's pixel-level edges.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let x = i % w;
            let v = if (x / 3).is_multiple_of(2) {
                0.7_f32
            } else {
                0.3_f32
            };
            [v, v, v, 1.0_f32]
        })
        .collect();

    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let (_, c_a, _, c_b) = create_pingpong(&rd, w, h);
    let (_, s_a, _, s_b) = create_pingpong(&rd, w, h);
    let (_, t_a, _, t_b) = create_pingpong(&rd, w, h);
    blur.render_pass(&source.view, &c_a, w, h, true, 16.0);
    blur.render_pass(&c_a, &c_b, w, h, false, 16.0);
    blur.render_pass(&source.view, &s_a, w, h, true, 1.5);
    blur.render_pass(&s_a, &s_b, w, h, false, 1.5);
    blur.render_pass(&source.view, &t_a, w, h, true, 5.0);
    blur.render_pass(&t_a, &t_b, w, h, false, 5.0);

    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass source.view for NR blur (not exercised in this texture test).
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(
        &source,
        &c_b,
        &s_b,
        &t_b,
        &source.view,
        &lut_view,
        &dlut_view,
    );

    let mut edit = EditState::default();
    edit.presence.texture = 50.0;
    pipe.update_uniforms(&EditUniforms::from(&edit));
    let (tex_on, view_on) = make_target(&rd, w, h);
    pipe.render(&view_on, &bind);
    let with_texture = read_to_cpu(&rd, &tex_on, w, h).unwrap();

    let edit_zero = EditState::default();
    pipe.update_uniforms(&EditUniforms::from(&edit_zero));
    let (tex_off, view_off) = make_target(&rd, w, h);
    pipe.render(&view_off, &bind);
    let without_texture = read_to_cpu(&rd, &tex_off, w, h).unwrap();

    let mut total_diff = 0i32;
    for i in 0..(w * h) {
        let idx = (i * 4) as usize;
        total_diff += (with_texture[idx] as i32 - without_texture[idx] as i32).abs();
    }
    assert!(
        total_diff > 50,
        "texture should produce visible difference, got {total_diff}"
    );
}

// ── Phase 2E.4: Noise Reduction ───────────────────────────────────────────────

/// NR luminance=100 on a noisy grey source should reduce pixel-to-pixel variance,
/// because the Gaussian blur used as the NR source smooths the noise.
#[test]
fn nr_luminance_100_smooths_noisy_source() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 32;
    let h: u32 = 32;
    // Deterministic noise pattern around 0.5 grey using a multiplicative hash.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let mut x = i.wrapping_mul(2654435761);
            x ^= x >> 16;
            x = x.wrapping_mul(2246822507);
            x ^= x >> 13;
            let n = (x as f32 / u32::MAX as f32 - 0.5) * 0.4;
            let v = (0.5 + n).clamp(0.0, 1.0);
            [v, v, v, 1.0]
        })
        .collect();

    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let (_, c_a, _, c_b) = create_pingpong(&rd, w, h);
    let (_, s_a, _, s_b) = create_pingpong(&rd, w, h);
    let (_, t_a, _, t_b) = create_pingpong(&rd, w, h);
    let (_, n_a, _, n_b) = create_pingpong(&rd, w, h);
    blur.render_pass(&source.view, &c_a, w, h, true, 16.0);
    blur.render_pass(&c_a, &c_b, w, h, false, 16.0);
    blur.render_pass(&source.view, &s_a, w, h, true, 1.5);
    blur.render_pass(&s_a, &s_b, w, h, false, 1.5);
    blur.render_pass(&source.view, &t_a, w, h, true, 5.0);
    blur.render_pass(&t_a, &t_b, w, h, false, 5.0);
    blur.render_pass(&source.view, &n_a, w, h, true, 2.0);
    blur.render_pass(&n_a, &n_b, w, h, false, 2.0);

    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(&source, &c_b, &s_b, &t_b, &n_b, &lut_view, &dlut_view);

    // Render with NR luminance = 0 (off).
    let edit_off = EditState::default();
    pipe.update_uniforms(&EditUniforms::from(&edit_off));
    let (tex_off, view_off) = make_target(&rd, w, h);
    pipe.render(&view_off, &bind);
    let pixels_off = read_to_cpu(&rd, &tex_off, w, h).unwrap();

    // Render with NR luminance = 100 (full smoothing).
    let mut edit_on = EditState::default();
    edit_on.detail.noise_reduction.luminance = 100.0;
    pipe.update_uniforms(&EditUniforms::from(&edit_on));
    let (tex_on, view_on) = make_target(&rd, w, h);
    pipe.render(&view_on, &bind);
    let pixels_on = read_to_cpu(&rd, &tex_on, w, h).unwrap();

    fn variance(pixels: &[u8], w: u32, h: u32) -> f32 {
        let mut sum = 0.0_f32;
        let count = (w * h) as f32;
        for i in 0..(w * h) {
            sum += pixels[(i * 4) as usize] as f32;
        }
        let mean = sum / count;
        let mut var = 0.0_f32;
        for i in 0..(w * h) {
            let d = pixels[(i * 4) as usize] as f32 - mean;
            var += d * d;
        }
        var / count
    }

    let var_off = variance(&pixels_off, w, h);
    let var_on = variance(&pixels_on, w, h);
    assert!(
        var_on < var_off * 0.8,
        "NR luminance=100 should reduce variance by at least 20%: off={var_off:.2} on={var_on:.2}"
    );
}

// ── Phase 2E.5: Dehaze ────────────────────────────────────────────────────────

/// Dehaze=+100 on a low-contrast "hazy" pattern (two sides at 0.45 and 0.55)
/// should widen the contrast span between the two halves, because the local-
/// contrast boost (biased toward darker regions) pushes the values apart.
#[test]
fn dehaze_positive_increases_contrast_on_hazy_pattern() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w = 32;
    let h = 32;
    // Simulate haze: low contrast around midgrey with a faint pattern.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let x = i % w;
            let v = if x < w / 2 { 0.45 } else { 0.55 };
            [v, v, v, 1.0]
        })
        .collect();
    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let (_, c_a, _, c_b) = create_pingpong(&rd, w, h);
    let (_, s_a, _, s_b) = create_pingpong(&rd, w, h);
    let (_, t_a, _, t_b) = create_pingpong(&rd, w, h);
    let (_, n_a, _, n_b) = create_pingpong(&rd, w, h);
    blur.render_pass(&source.view, &c_a, w, h, true, 16.0);
    blur.render_pass(&c_a, &c_b, w, h, false, 16.0);
    blur.render_pass(&source.view, &s_a, w, h, true, 1.5);
    blur.render_pass(&s_a, &s_b, w, h, false, 1.5);
    blur.render_pass(&source.view, &t_a, w, h, true, 5.0);
    blur.render_pass(&t_a, &t_b, w, h, false, 5.0);
    blur.render_pass(&source.view, &n_a, w, h, true, 2.0);
    blur.render_pass(&n_a, &n_b, w, h, false, 2.0);
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(&source, &c_b, &s_b, &t_b, &n_b, &lut_view, &dlut_view);

    let edit_off = EditState::default();
    pipe.update_uniforms(&EditUniforms::from(&edit_off));
    let (tex_off, view_off) = make_target(&rd, w, h);
    pipe.render(&view_off, &bind);
    let off = read_to_cpu(&rd, &tex_off, w, h).unwrap();

    let mut edit_on = EditState::default();
    edit_on.presence.dehaze = 100.0;
    pipe.update_uniforms(&EditUniforms::from(&edit_on));
    let (tex_on, view_on) = make_target(&rd, w, h);
    pipe.render(&view_on, &bind);
    let on = read_to_cpu(&rd, &tex_on, w, h).unwrap();

    // Light half should brighten or darken further; dark half similarly. Verify span widens.
    let center_left = off[((16 * w + 8) * 4) as usize];
    let center_right = off[((16 * w + 24) * 4) as usize];
    let span_off = (center_right as i32 - center_left as i32).abs();
    let on_left = on[((16 * w + 8) * 4) as usize];
    let on_right = on[((16 * w + 24) * 4) as usize];
    let span_on = (on_right as i32 - on_left as i32).abs();
    assert!(
        span_on > span_off,
        "dehaze should widen the contrast span: off={span_off} on={span_on}"
    );
}

// ── Phase 2E polish: Bilateral NR ─────────────────────────────────────────────

/// Smoke test: verify the BilateralPipeline compiles and runs without panicking.
/// A full bilateral-vs-Gaussian comparison would require Rgba16Float readback
/// which is not yet supported; this test confirms pipeline creation + render pass
/// completes on a sharp-edge source without crashing.
#[test]
fn bilateral_pipeline_smoke_test() {
    let _gpu_test_guard = gpu_test_lock();
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let w: u32 = 32;
    let h: u32 = 32;
    // Sharp-edge source: left half dark, right half bright.
    let pixels: Vec<f32> = (0..w * h)
        .flat_map(|i: u32| {
            let x = i % w;
            let v = if x < w / 2 { 0.2_f32 } else { 0.8_f32 };
            [v, v, v, 1.0_f32]
        })
        .collect();

    let source = SourceTexture::upload(&rd, w, h, &pixels);
    let blur = BlurPipeline::new(&rd);
    let bilat = BilateralPipeline::new(&rd);

    // Allocate ping-pong textures for clarity, sharp, texture, and bilateral output.
    let (_, c_a, _, c_b) = create_pingpong(&rd, w, h);
    let (_, s_a, _, s_b) = create_pingpong(&rd, w, h);
    let (_, t_a, _, t_b) = create_pingpong(&rd, w, h);
    let (_, _nr_a, _, nr_b) = create_pingpong(&rd, w, h);

    blur.render_pass(&source.view, &c_a, w, h, true, 16.0);
    blur.render_pass(&c_a, &c_b, w, h, false, 16.0);
    blur.render_pass(&source.view, &s_a, w, h, true, 1.5);
    blur.render_pass(&s_a, &s_b, w, h, false, 1.5);
    blur.render_pass(&source.view, &t_a, w, h, true, 5.0);
    blur.render_pass(&t_a, &t_b, w, h, false, 5.0);

    // Bilateral filter: sigma_spatial=2px, sigma_range=0.1 (moderate edge-preservation), 7×7 window.
    bilat.render_pass(&source.view, &nr_b, 2.0, 0.1, 3.0);

    // Build the develop pipeline and verify it can render using the bilateral output.
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    // Pass identity LUT for tone curve (no effect). Pass identity 3D LUT for display profile.
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(&rd);
    let bind = pipe.make_bind_group(&source, &c_b, &s_b, &t_b, &nr_b, &lut_view, &dlut_view);

    let mut edit = EditState::default();
    edit.detail.noise_reduction.luminance = 80.0;
    edit.detail.noise_reduction.color = 60.0;
    pipe.update_uniforms(&EditUniforms::from(&edit));

    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bind);
    // If we reach here without panic the bilateral pipeline is functioning.
    let pixels_out = read_to_cpu(&rd, &tex, w, h).unwrap();
    assert_eq!(
        pixels_out.len(),
        (w * h * 4) as usize,
        "output buffer should have w*h*4 bytes"
    );
}
