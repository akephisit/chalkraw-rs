// chalkraw-rs develop shader — Phase 1 implements Exposure only.
// Later phases extend this single fused per-pixel shader with the remaining
// basic, tone-curve, HSL, color-grading, presence, and effects operations.

struct EditUniforms {
    exposure: f32,
    _pad_tone: vec3<f32>,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    _pad_basic: vec3<f32>,
    temp_kelvin: f32,
    tint: f32,
    _pad_wb: vec2<f32>,
    vibrance: f32,
    saturation: f32,
    texture: f32,
    clarity: f32,
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
    let src = textureSample(source_tex, source_sampler, in.uv);

    // Exposure: multiply linear by 2^stops.
    let gain = pow(2.0, edit.exposure);
    let lit = src.rgb * gain;

    // Output is linear; final sRGB encode is handled by the target view format.
    return vec4<f32>(lit, src.a);
}
