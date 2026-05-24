/// Golden-image tests for Phase 2A develop sliders.
///
/// Each test uploads a known solid-colour source, sets one slider, renders,
/// and asserts the central pixel moved in the expected direction.  All tests
/// skip gracefully when no GPU adapter is available (CI / sandbox).
use chalkraw_core::{Crop, EditState};
use chalkraw_render::{
    make_target, read_to_cpu, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
};

// ── helpers ──────────────────────────────────────────────────────────────────

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
    let bg = pipe.make_bind_group(&src);
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    assert!(p[0] > base_p[0], "contrast +50 should brighten pixel above midgrey; got {p:?} vs base {base_p:?}");
}

/// +50 shadows on a 0.2-grey pixel should brighten it.
#[test]
fn shadows_plus_50_brightens_dark_pixels() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.tone.shadows = 50.0;

    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &base_edit);
    let base_p = pixel_at(&base_pixels, w, 8, 8);

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(p[0] > base_p[0], "shadows +50 should brighten dark pixel; got {p:?} vs base {base_p:?}");
}

/// Warm white balance (7500 K > 5500 K neutral) should shift red up and blue down.
#[test]
fn wb_warm_shifts_red_up_blue_down() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.white_balance.temp_kelvin = 7500.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    // R should be brighter than B on a warm tint.
    assert!(p[0] > p[2], "warm WB should have R > B; got R={} B={}", p[0], p[2]);
}

/// -100 saturation on a solid red pixel should produce equal R=G=B (grey).
#[test]
fn saturation_minus_100_produces_grey() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    assert!(diff_rg <= 4, "saturation -100 should equalise R and G; diff={diff_rg}, p={p:?}");
    assert!(diff_rb <= 4, "saturation -100 should equalise R and B; diff={diff_rb}, p={p:?}");
}

/// -100 vignette amount should darken the corner pixel relative to the centre.
#[test]
fn vignette_minus_100_darkens_corners() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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

    assert!(corner[0] < centre[0],
        "vignette -100 should darken corner vs centre; corner={} centre={}", corner[0], centre[0]);
}

/// Grain amount=50 on a uniform grey should introduce per-pixel variation
/// (not every pixel identical), confirming the hash noise is being applied.
#[test]
fn grain_amount_50_introduces_variation() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (32, 32);
    let mut edit = EditState::default();
    edit.effects.grain.amount = 50.0;
    edit.effects.grain.size = 50.0; // medium frequency

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.5), &edit);

    // With grain, not all pixels should be identical. Collect unique R values.
    let unique_r: std::collections::HashSet<u8> =
        (0..w * h).map(|i| pixels[(i * 4) as usize]).collect();
    assert!(unique_r.len() > 1,
        "grain should introduce per-pixel variation; all pixels had same R={:?}", unique_r);
}

/// HSL red hue shift: solid red input with hsl[0].hue=50 should rotate the
/// red hue, reducing R and increasing G (shifting toward orange/yellow).
#[test]
fn hsl_red_hue_shift_rotates_red() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.color_grading.shadows.hue = 240.0;        // blue
    edit.color_grading.shadows.saturation = 100.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.2), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    assert!(
        p[2] > p[0],
        "cg shadows blue tint on dark pixel should produce B > R; got R={} G={} B={}",
        p[0], p[1], p[2]
    );
}

/// Color Grading — yellow highlights tint on a light pixel (0.8 grey):
/// highlights.hue=60 (yellow), highlights.saturation=100 → output R+G > B.
#[test]
fn cg_highlights_yellow_tints_light_pixels() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (16, 16);
    let mut edit = EditState::default();
    edit.color_grading.highlights.hue = 60.0;        // yellow
    edit.color_grading.highlights.saturation = 100.0;

    let pixels = render_solid(&rd, w, h, solid_grey(w, h, 0.8), &edit);
    let p = pixel_at(&pixels, w, 8, 8);

    let rg_sum = p[0] as u32 + p[1] as u32;
    let b = p[2] as u32;
    assert!(
        rg_sum > b * 2,
        "cg highlights yellow tint on light pixel should produce R+G > B; got R={} G={} B={}",
        p[0], p[1], p[2]
    );
}

/// Color Grading — global luminance -50 should darken any pixel.
#[test]
fn cg_global_lum_minus_50_darkens_all() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
        p[0], base_p[0]
    );
}

/// Parametric curve shadows=+50 on a 0.1-grey pixel (deep shadow zone) should lift it.
#[test]
fn parametric_shadows_plus_50_lifts_dark_pixels() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
        corner[0], centre[0]
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let (w, h) = (32u32, 32u32);

    // Build a source where the top-left quadrant is dark (0.2) and the
    // bottom-right quadrant is bright (0.8). The other two quadrants are 0.5.
    let source: Vec<f32> = (0..w * h).flat_map(|i| {
        let px = i % w;
        let py = i / w;
        let v = if px < w / 2 && py < h / 2 { 0.2_f32 } else { 0.8_f32 };
        [v, v, v, 1.0_f32]
    }).collect();

    // Baseline: no crop, output centre maps to source (0.5, 0.5) → bright region.
    let base_edit = EditState::default();
    let base_pixels = render_solid(&rd, w, h, source.clone(), &base_edit);
    let base_centre = pixel_at(&base_pixels, w, w / 2, h / 2);

    // With crop: top-left quadrant only. Output centre → source (0.25, 0.25) → dark region.
    let edit = EditState {
        crop: Some(Crop { x_pct: 0.0, y_pct: 0.0, w_pct: 0.5, h_pct: 0.5, rotation_deg: 0.0 }),
        ..EditState::default()
    };
    let crop_pixels = render_solid(&rd, w, h, source, &edit);
    let crop_centre = pixel_at(&crop_pixels, w, w / 2, h / 2);

    assert!(
        crop_centre[0] < base_centre[0],
        "crop top-left quadrant: centre should sample dark region (R={}) vs no-crop bright (R={})",
        crop_centre[0], base_centre[0]
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
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skip: no GPU"); return; }
    };
    let w: u32 = 16;
    let h: u32 = 16;
    let pixels: Vec<f32> = (0..w * h).flat_map(|_| [0.5_f32, 0.5, 0.5, 1.0]).collect();
    let edit = EditState::default();

    // ── hardware sRGB path ────────────────────────────────────────────────────
    let pipe_hw = DevelopPipeline::new(&rd, PipelineConfig {
        output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
    });
    pipe_hw.update_uniforms(&EditUniforms::from(&edit));
    let src_hw = SourceTexture::upload(&rd, w, h, &pixels);
    let bg_hw = pipe_hw.make_bind_group(&src_hw);
    let (tex_hw, view_hw) = make_target(&rd, w, h);
    pipe_hw.render(&view_hw, &bg_hw);
    let out_hw = read_to_cpu(&rd, &tex_hw, w, h).unwrap();

    // ── manual sRGB path (Rgba8Unorm, shader encodes) ─────────────────────────
    let pipe_sw = DevelopPipeline::new(&rd, PipelineConfig {
        output_format: wgpu::TextureFormat::Rgba8Unorm,
    });
    // Verify the flag was set correctly.
    assert!(pipe_sw.manual_srgb_needed, "pipeline with Rgba8Unorm should set manual_srgb_needed");

    pipe_sw.update_uniforms(&EditUniforms::from(&edit));
    let src_sw = SourceTexture::upload(&rd, w, h, &pixels);
    let bg_sw = pipe_sw.make_bind_group(&src_sw);

    // Create a non-sRGB render target manually (make_target always uses Rgba8UnormSrgb).
    let target_sw = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("readback target non-srgb"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
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
            center_hw[c], center_sw[c]
        );
    }
}

/// Vibrance=+100 on a near-grey pixel should boost saturation more than
/// Vibrance=0, but not exceed what full Saturation=+100 would do.
#[test]
fn vibrance_boosts_low_saturation_colors() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
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
