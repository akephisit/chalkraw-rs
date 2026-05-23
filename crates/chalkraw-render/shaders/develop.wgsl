// chalkraw-rs develop shader — Phase 2A: per-pixel basic develop sliders.
// Operations are applied in Lightroom order: WB → Exposure → Contrast →
// Highlights/Shadows/Whites/Blacks → Saturation → Vibrance → Vignette → Grain.
//
// The WGSL struct layout must stay byte-for-byte in sync with EditUniforms in
// uniforms.rs (total 128 bytes = 32 f32 values). WGSL alignment rules:
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
// Total: 128 bytes.

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
};

@group(0) @binding(0) var source_tex: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> edit: EditUniforms;

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
    var rgb = textureSample(source_tex, source_sampler, in.uv).rgb;
    let a = textureSample(source_tex, source_sampler, in.uv).a;

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

    // 6. Vibrance — saturation boost weighted against already-saturated colours.
    let max_c = max(max(rgb.r, rgb.g), rgb.b);
    let min_c = min(min(rgb.r, rgb.g), rgb.b);
    let cur_sat = max_c - min_c;
    let vib_weight = 1.0 - clamp(cur_sat, 0.0, 1.0);
    let gray_vib = vec3<f32>(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
    rgb = mix(gray_vib, rgb, 1.0 + edit.vibrance / 100.0 * vib_weight);

    // 7. Vignette — radial darkening around image centre.
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

    // 8. Grain — single-octave hash noise.
    //    grain_roughness is passed through to the uniform buffer but has no
    //    shader effect in Phase 2A; reserved for multi-octave noise in Phase 2E.
    let scale = mix(50.0, 500.0, 1.0 - edit.grain_size / 100.0);
    let h = fract(sin(dot(in.uv * scale, vec2<f32>(127.1, 311.7))) * 43758.5453);
    let noise = (h - 0.5) * edit.grain_amount / 100.0 * 0.3;
    rgb += vec3<f32>(noise);

    return vec4<f32>(max(rgb, vec3<f32>(0.0)), a);
}
