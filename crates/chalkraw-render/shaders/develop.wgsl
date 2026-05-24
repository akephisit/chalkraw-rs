// chalkraw-rs develop shader — Phase 2F: Lens Correction + Slider-based Crop.
// Operations are applied in Lightroom order: WB → Exposure → Contrast →
// Highlights/Shadows/Whites/Blacks → Saturation → HSL → Color Grading →
// Parametric Curve → Vibrance → Vignette → Grain.
//
// The WGSL struct layout must stay byte-for-byte in sync with EditUniforms in
// uniforms.rs (total 304 bytes). WGSL alignment rules:
//   vec4<f32> → align 16, size 16
//   vec3<f32> → align 16, size 12
//   vec2<f32> → align  8, size  8
//   f32       → align  4, size  4
//
// Annotated offsets (WGSL applies implicit inter-field padding):
//   0   exposure       f32
//   4   (implicit 12-byte pad before the vec3)
//  16   _pad_tone      vec3<f32>   → covers Rust _pre_pad + _pad_tone
//  28   contrast       f32
//  32   highlights     f32
//  36   shadows        f32
//  40   whites         f32
//  44   blacks         f32
//  48   _pad_basic     vec3<f32>
//  60   temp_kelvin    f32
//  64   tint           f32
//  68   (implicit 4-byte pad before the vec2)
//  72   _pad_wb        vec2<f32>
//  80   vibrance       f32
//  84   saturation     f32
//  88   texture        f32
//  92   clarity        f32
//  96   vignette_amount    f32
// 100   vignette_midpoint  f32
// 104   vignette_feather   f32
// 108   vignette_roundness f32
// 112   grain_amount       f32
// 116   grain_size         f32
// 120   grain_roughness    f32   (reserved; no shader effect in Phase 2A)
// 124   _pad_grain         f32
// Phase 2B (HSL): 8 colors × {hue, sat, lum}, 6 × vec4<f32> = 96 bytes.
// 128   hsl_hue_a      vec4<f32>   red, orange, yellow, green
// 144   hsl_hue_b      vec4<f32>   aqua, blue, purple, magenta
// 160   hsl_sat_a      vec4<f32>
// 176   hsl_sat_b      vec4<f32>
// 192   hsl_lum_a      vec4<f32>
// 208   hsl_lum_b      vec4<f32>
// Phase 2C (Color Grading): 4 regions × {hue, sat, lum} + blending/balance, 4 × vec4<f32> = 64 bytes.
// 224   cg_hue         vec4<f32>   hue for [shadows, midtones, highlights, global]
// 240   cg_sat         vec4<f32>   saturation for [shadows, midtones, highlights, global]
// 256   cg_lum         vec4<f32>   luminance for [shadows, midtones, highlights, global]
// 272   cg_blend_balance vec4<f32> [blending, balance, 0, 0]
// Phase 2D (Parametric Curve): 1 × vec4<f32> = 16 bytes.
// 288   param_curve    vec4<f32>   [shadows, darks, lights, highlights]
// Phase 2F (Lens Correction): 2 × f32 + vec2<f32> pad = 16 bytes.
// 304   lens_distortion  f32
// 308   lens_vignetting  f32
// 312   _pad_lens        vec2<f32>
// Phase 2F (Crop): 6 × f32 + vec2<f32> pad = 32 bytes.
// 320   crop_enabled     f32
// 324   crop_x           f32
// 328   crop_y           f32
// 332   crop_w           f32
// 336   crop_h           f32
// 340   crop_rotation_deg f32
// 344   _pad_crop        vec2<f32>
// Phase 0.13.2 (manual sRGB): vec4<u32> = 16 bytes.
// 352   srgb_pad         vec4<u32>   .x = srgb_output flag (1 = encode, 0 = skip)
// Total: 368 bytes.

struct EditUniforms {
    exposure:           f32,
    _pad_tone:          vec3<f32>,   // align 16 → offset 16; covers _pre_pad+_pad_tone in Rust
    contrast:           f32,
    highlights:         f32,
    shadows:            f32,
    whites:             f32,
    blacks:             f32,
    _pad_basic:         vec3<f32>,   // align 16 → offset 48; covers _pad_basic in Rust
    temp_kelvin:        f32,
    tint:               f32,
    _pad_wb:            vec2<f32>,   // align 8 → offset 72; implicit 4-byte pad at 68
    vibrance:           f32,
    saturation:         f32,
    texture:            f32,
    clarity:            f32,
    vignette_amount:    f32,
    vignette_midpoint:  f32,
    vignette_feather:   f32,
    vignette_roundness: f32,
    grain_amount:       f32,
    grain_size:         f32,
    grain_roughness:    f32,
    _pad_grain:         f32,
    // Phase 2B: HSL per-band adjustments (6 × vec4<f32>, offset 128..224).
    hsl_hue_a:          vec4<f32>,   // red, orange, yellow, green
    hsl_hue_b:          vec4<f32>,   // aqua, blue, purple, magenta
    hsl_sat_a:          vec4<f32>,
    hsl_sat_b:          vec4<f32>,
    hsl_lum_a:          vec4<f32>,
    hsl_lum_b:          vec4<f32>,
    // Phase 2C: Color Grading (4 × vec4<f32>, offset 224..288).
    cg_hue:             vec4<f32>,   // hue for [shadows, midtones, highlights, global]
    cg_sat:             vec4<f32>,   // saturation for [shadows, midtones, highlights, global]
    cg_lum:             vec4<f32>,   // luminance for [shadows, midtones, highlights, global]
    cg_blend_balance:   vec4<f32>,   // [blending, balance, 0, 0]
    // Phase 2D: Parametric Curve (1 × vec4<f32>, offset 288..304).
    param_curve:        vec4<f32>,   // [shadows, darks, lights, highlights]
    // Phase 2F: Lens Correction (offset 304..320).
    lens_distortion:    f32,
    lens_vignetting:    f32,
    _pad_lens:          vec2<f32>,
    // Phase 2F: Crop (offset 320..352).
    crop_enabled:       f32,
    crop_x:             f32,
    crop_y:             f32,
    crop_w:             f32,
    crop_h:             f32,
    crop_rotation_deg:  f32,
    _pad_crop:          vec2<f32>,
    // Phase 0.13.2: manual sRGB encode flag (offset 352..368).
    // Packed as vec4<u32> so the WGSL struct size matches the Rust layout exactly:
    // .x = srgb_output (1 = manual encode, 0 = hardware does it), .yzw = padding.
    srgb_pad:           vec4<u32>,
};

@group(0) @binding(0) var source_tex: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> edit: EditUniforms;
// Phase 2E.1: pre-blurred source texture for Clarity local-contrast boost.
@group(0) @binding(3) var blur_tex: texture_2d<f32>;

// ── HSV conversion helpers (Sam Hocevar's branchless algorithm) ───────────────

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, K.wz), vec4<f32>(c.gb, K.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

// ── Parametric Curve helper ───────────────────────────────────────────────────

// Per-zone additive lift for one channel value v (0..1).
// p.x = shadows (-100..100), p.y = darks, p.z = lights, p.w = highlights.
// Zone edges: 0.0, 0.33, 0.66, 1.0.  Smooth overlap at boundaries.
fn parametric_zone_lift(v: f32, p: vec4<f32>) -> f32 {
    let sh = 1.0 - smoothstep(0.0, 0.33, v);
    let hi = smoothstep(0.66, 1.0, v);
    let dk = smoothstep(0.0, 0.33, v) * (1.0 - smoothstep(0.33, 0.66, v));
    let lt = smoothstep(0.33, 0.66, v) * (1.0 - smoothstep(0.66, 1.0, v));
    return (sh * p.x + dk * p.y + lt * p.z + hi * p.w) / 100.0 * 0.25;
}

// ── Color Grading helper ──────────────────────────────────────────────────────

// Convert a (hue_deg, sat) pair into a small additive RGB tint offset.
// hue_deg is 0..360, sat is 0..100.  The (rgb - 0.5) * scale approach gives
// a Lightroom-style additive colour grading tint rather than a full hue rotate.
fn cg_tint_rgb(hue_deg: f32, sat: f32) -> vec3<f32> {
    let h01 = fract(hue_deg / 360.0);
    let rgb = hsv_to_rgb(vec3<f32>(h01, 1.0, 1.0));
    let neutral = vec3<f32>(0.5);
    return (rgb - neutral) * (sat / 100.0) * 0.25;
}

// ── IEC 61966-2-1 linear→sRGB encode ─────────────────────────────────────────
// Used when the output surface is not sRGB-coded (e.g. Bgra8Unorm). Input must
// be clamped to [0, 1] before calling to keep pow() well-defined.
fn linear_to_srgb(c: f32) -> f32 {
    if (c <= 0.0031308) {
        return c * 12.92;
    } else {
        return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    }
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
    // Full-screen triangle covering NDC.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    let p = positions[idx];
    let uv = uvs[idx];
    var out: VertexOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Phase 2F: crop maps output UV (0..1) → source UV inside the crop rect.
    // Plus rotation around the canvas centre (degrees → radians).
    var uv = in.uv;
    if (edit.crop_enabled > 0.5) {
        let rad = edit.crop_rotation_deg * 3.14159265 / 180.0;
        let cs = cos(rad);
        let sn = sin(rad);
        // Centre output around (0.5, 0.5) so rotation pivots at canvas centre.
        let centred = uv - vec2<f32>(0.5);
        let rotated = vec2<f32>(centred.x * cs - centred.y * sn,
                                centred.x * sn + centred.y * cs);
        let recentred = rotated + vec2<f32>(0.5);
        // Map recentred (0..1) into the crop rectangle inside source.
        uv = vec2<f32>(edit.crop_x + recentred.x * edit.crop_w,
                       edit.crop_y + recentred.y * edit.crop_h);
    }

    // Phase 2F: lens distortion. Positive = barrel (pull outwards), negative = pincushion.
    let centre_uv = vec2<f32>(0.5);
    let r2 = dot(uv - centre_uv, uv - centre_uv);
    let k = edit.lens_distortion / 100.0 * 0.5;
    uv = centre_uv + (uv - centre_uv) * (1.0 + k * r2);

    let sample = textureSample(source_tex, source_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    var rgb = sample.rgb;
    let a = sample.a;

    // 1. White Balance — simple per-channel temperature/tint multipliers.
    //    delta_k ≈ [-0.7, 0.7] across the typical 2000–10000 K range.
    let delta_k = (edit.temp_kelvin - 5500.0) / 5500.0;
    rgb.r *= 1.0 + delta_k * 0.5;
    rgb.b *= 1.0 - delta_k * 0.5;
    rgb.g *= 1.0 + edit.tint / 100.0 * 0.3;

    // 2. Exposure — multiply linear by 2^stops.
    rgb *= pow(2.0, edit.exposure);

    // 3. Contrast — pivot around 0.5 in linear light.
    rgb = (rgb - 0.5) * (1.0 + edit.contrast / 100.0) + 0.5;

    // 4. Highlights / Shadows / Whites / Blacks — luminance-weighted gains.
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let shadow_mask    = smoothstep(0.5, 0.0, lum);
    let highlight_mask = smoothstep(0.5, 1.0, lum);
    let black_mask     = smoothstep(0.2, 0.0, lum);
    let white_mask     = smoothstep(0.8, 1.0, lum);
    rgb *= 1.0 + shadow_mask    * edit.shadows    / 100.0;
    rgb *= 1.0 + highlight_mask * edit.highlights / 100.0;
    rgb *= 1.0 + black_mask     * edit.blacks     / 100.0;
    rgb *= 1.0 + white_mask     * edit.whites     / 100.0;

    // 5. Saturation — blend toward/away from luminance grey.
    let gray_sat = vec3<f32>(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
    rgb = mix(gray_sat, rgb, 1.0 + edit.saturation / 100.0);

    // 5b. HSL — per-band hue/saturation/luminance adjustments.
    //     Lightroom-style: each of 8 colour bands has a linear falloff around its
    //     centre hue (band_width = 45°). Pixels near the centre get the full
    //     adjustment; pixels outside the band get none.
    {
        let hsv = rgb_to_hsv(rgb);
        let h_deg = hsv.x * 360.0;

        // Band centres in degrees (must match EditState hsl array order).
        let centres = array<f32, 8>(0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0);

        // Unpack the 6 vec4 chunks into per-band arrays.
        let hues = array<f32, 8>(
            edit.hsl_hue_a.x, edit.hsl_hue_a.y, edit.hsl_hue_a.z, edit.hsl_hue_a.w,
            edit.hsl_hue_b.x, edit.hsl_hue_b.y, edit.hsl_hue_b.z, edit.hsl_hue_b.w,
        );
        let sats = array<f32, 8>(
            edit.hsl_sat_a.x, edit.hsl_sat_a.y, edit.hsl_sat_a.z, edit.hsl_sat_a.w,
            edit.hsl_sat_b.x, edit.hsl_sat_b.y, edit.hsl_sat_b.z, edit.hsl_sat_b.w,
        );
        let lums = array<f32, 8>(
            edit.hsl_lum_a.x, edit.hsl_lum_a.y, edit.hsl_lum_a.z, edit.hsl_lum_a.w,
            edit.hsl_lum_b.x, edit.hsl_lum_b.y, edit.hsl_lum_b.z, edit.hsl_lum_b.w,
        );

        var hue_shift: f32 = 0.0;
        var sat_scale: f32 = 0.0;
        var lum_scale: f32 = 0.0;
        let band_width = 45.0; // degrees half-band; beyond this the weight is 0

        for (var i: u32 = 0u; i < 8u; i = i + 1u) {
            // Smallest circular angular distance between pixel hue and band centre.
            var d = abs(h_deg - centres[i]);
            d = min(d, 360.0 - d);
            // Linear tent weight: 1 at centre, 0 at band edge.
            let w = max(0.0, 1.0 - d / band_width);
            hue_shift = hue_shift + w * hues[i];
            sat_scale = sat_scale + w * sats[i];
            lum_scale = lum_scale + w * lums[i];
        }

        // Apply accumulated adjustments.
        // Hue slider ±100 → ±36° rotation (0.36° per unit).
        var hsv_out = hsv;
        hsv_out.x = fract((h_deg + hue_shift * 0.36) / 360.0);
        hsv_out.y = clamp(hsv_out.y * (1.0 + sat_scale / 100.0), 0.0, 1.0);
        hsv_out.z = clamp(hsv_out.z * (1.0 + lum_scale / 100.0), 0.0, 1.0);
        rgb = hsv_to_rgb(hsv_out);
    }

    // 6. Color Grading — per-region tint (shadows/midtones/highlights/global) plus
    //    per-region luminance offsets. Weights driven by pixel luminance with
    //    smoothstep falloff; balance slides the shadow/highlight pivot.
    {
        let cg_lum_in = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let balance   = edit.cg_blend_balance.y / 100.0;   // -1..1
        let blend     = edit.cg_blend_balance.x / 100.0;   //  0..1 (default 0.5)
        let pivot_low  = 0.25 + balance * 0.25;
        let pivot_high = 0.75 + balance * 0.25;
        let feather    = mix(0.05, 0.4, clamp(blend, 0.0, 1.0));

        let sh_w  = 1.0 - smoothstep(pivot_low - feather, pivot_low + feather, cg_lum_in);
        let hi_w  = smoothstep(pivot_high - feather, pivot_high + feather, cg_lum_in);
        let mid_w = max(0.0, 1.0 - sh_w - hi_w);

        // Per-region tint offsets.
        let sh_tint  = cg_tint_rgb(edit.cg_hue.x, edit.cg_sat.x);
        let mid_tint = cg_tint_rgb(edit.cg_hue.y, edit.cg_sat.y);
        let hi_tint  = cg_tint_rgb(edit.cg_hue.z, edit.cg_sat.z);
        let gl_tint  = cg_tint_rgb(edit.cg_hue.w, edit.cg_sat.w);

        rgb = rgb + sh_tint * sh_w + mid_tint * mid_w + hi_tint * hi_w + gl_tint;

        // Per-region luminance shift (-100..100 each); halved to avoid blow-out.
        let lum_shift =
              edit.cg_lum.x * sh_w
            + edit.cg_lum.y * mid_w
            + edit.cg_lum.z * hi_w
            + edit.cg_lum.w;
        rgb = rgb * (1.0 + lum_shift / 100.0 * 0.5);
    }

    // 6b. Parametric Tone Curve — per-zone lift, smooth falloff at zone boundaries.
    //     Applied per channel independently (same curve for R, G, B).
    {
        let pc = edit.param_curve;
        rgb.r = clamp(rgb.r + parametric_zone_lift(rgb.r, pc), 0.0, 1.0);
        rgb.g = clamp(rgb.g + parametric_zone_lift(rgb.g, pc), 0.0, 1.0);
        rgb.b = clamp(rgb.b + parametric_zone_lift(rgb.b, pc), 0.0, 1.0);
    }

    // 6c. Clarity — local-contrast enhancement using the pre-blurred source.
    //     blurred is sampled at the *output* UV (after crop/lens), which is the same
    //     UV used to sample source_tex, ensuring the blur matches the geometry.
    //     clarity / 100 * 0.5 keeps amount=100 from blowing out.
    {
        let blurred = textureSample(blur_tex, source_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0))).rgb;
        let local_contrast = rgb - blurred;
        rgb = rgb + local_contrast * (edit.clarity / 100.0 * 0.5);
    }

    // 7. Vibrance — saturation boost weighted against already-saturated colours.
    let max_c = max(max(rgb.r, rgb.g), rgb.b);
    let min_c = min(min(rgb.r, rgb.g), rgb.b);
    let cur_sat = max_c - min_c;
    let vib_weight = 1.0 - clamp(cur_sat, 0.0, 1.0);
    let gray_vib = vec3<f32>(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
    rgb = mix(gray_vib, rgb, 1.0 + edit.vibrance / 100.0 * vib_weight);

    // 8. Vignette — radial darkening around image centre.
    //    vignette_roundness shifts the Minkowski metric: p=2 (circular) to p=8
    //    (more square). Aspect-ratio correction is deferred to Phase 2F.
    let centre = vec2<f32>(0.5, 0.5);
    let r = abs(in.uv - centre) * 2.0;
    let p = mix(2.0, 8.0, clamp((edit.vignette_roundness + 100.0) / 200.0, 0.0, 1.0));
    let dist = pow(pow(r.x, p) + pow(r.y, p), 1.0 / p);
    let midpoint01 = edit.vignette_midpoint / 100.0;
    let feather01  = max(edit.vignette_feather / 100.0, 0.001);
    let vig_mask   = smoothstep(midpoint01, midpoint01 + feather01, dist);
    let vig_factor = 1.0 + vig_mask * edit.vignette_amount / 100.0;
    rgb *= vig_factor;

    // 9. Grain — single-octave hash noise.
    //    grain_roughness is passed through to the uniform buffer but has no
    //    shader effect in Phase 2A; reserved for multi-octave noise in Phase 2E.
    let scale = mix(50.0, 500.0, 1.0 - edit.grain_size / 100.0);
    let h = fract(sin(dot(in.uv * scale, vec2<f32>(127.1, 311.7))) * 43758.5453);
    let noise = (h - 0.5) * edit.grain_amount / 100.0 * 0.3;
    rgb += vec3<f32>(noise);

    // Phase 2F: lens vignetting correction — radial brightening to compensate
    // physical falloff. Distinct from Effects.Vignette which is creative darkening.
    let r_corr = length(uv - centre_uv) * 1.41421356;  // sqrt(2) normalises corner to 1.0
    let lv_amount = edit.lens_vignetting / 100.0;
    rgb *= 1.0 + lv_amount * r_corr * r_corr * 0.5;

    // Phase 0.13.2: manual sRGB encode — only when the output surface is not
    // sRGB-coded (e.g. Bgra8Unorm on Windows). Clamp first to keep pow() safe.
    // srgb_pad.x = srgb_output flag (1 = manual encode).
    if (edit.srgb_pad.x == 1u) {
        rgb = vec3<f32>(
            linear_to_srgb(clamp(rgb.r, 0.0, 1.0)),
            linear_to_srgb(clamp(rgb.g, 0.0, 1.0)),
            linear_to_srgb(clamp(rgb.b, 0.0, 1.0)),
        );
    }

    return vec4<f32>(max(rgb, vec3<f32>(0.0)), a);
}
