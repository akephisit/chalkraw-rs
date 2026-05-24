use chalkraw_core::EditState;
use chalkraw_render::{
    make_identity_lut, make_target, read_to_cpu, DevelopPipeline, EditUniforms, PipelineConfig,
    RenderDevice, SourceTexture,
};

fn solid_image(w: u32, h: u32, gray: f32) -> Vec<f32> {
    (0..w * h).flat_map(|_| [gray, gray, gray, 1.0]).collect()
}

fn pixel_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn exposure_zero_returns_input_brightness() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let w = 16;
    let h = 16;
    let src = SourceTexture::upload(&rd, w, h, &solid_image(w, h, 0.5));
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    let mut edit = EditState::default();
    edit.tone.exposure = 0.0;
    pipe.update_uniforms(&EditUniforms::from(&edit));
    // Pass source.view for all blur views (Clarity/Sharpening/Texture/NR not exercised).
    // Pass identity LUT for tone curve (no effect).
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let bg = pipe.make_bind_group(&src, &src.view, &src.view, &src.view, &src.view, &lut_view);
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bg);
    let pixels = read_to_cpu(&rd, &tex, w, h).unwrap();
    // Linear 0.5 → sRGB ~0.735 → 187. Allow tolerance for f16 + sRGB conversion.
    let p = pixel_at(&pixels, w, 8, 8);
    assert!((180..=195).contains(&(p[0] as u32)), "got R={}", p[0]);
}

#[test]
fn exposure_plus_one_doubles_linear_brightness() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let w = 16;
    let h = 16;
    let src = SourceTexture::upload(&rd, w, h, &solid_image(w, h, 0.25));
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    let mut edit = EditState::default();
    edit.tone.exposure = 1.0; // 2× linear
    pipe.update_uniforms(&EditUniforms::from(&edit));
    // Pass source.view for all blur views (Clarity/Sharpening/Texture/NR not exercised).
    // Pass identity LUT for tone curve (no effect).
    let (_lut_tex, lut_view) = make_identity_lut(&rd);
    let bg = pipe.make_bind_group(&src, &src.view, &src.view, &src.view, &src.view, &lut_view);
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bg);
    let pixels = read_to_cpu(&rd, &tex, w, h).unwrap();
    // Linear 0.5 → sRGB byte ~187.
    let p = pixel_at(&pixels, w, 8, 8);
    assert!((180..=195).contains(&(p[0] as u32)), "got R={}", p[0]);
}
