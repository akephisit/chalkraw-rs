struct BlurUniforms {
    direction: vec2<f32>,   // (1/w, 0) for horizontal, (0, 1/h) for vertical
    sigma: f32,
    radius: f32,            // typically ceil(3 * sigma)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: BlurUniforms;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
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
    let sigma = max(u.sigma, 0.001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    var accum = vec4<f32>(0.0);
    var weight_sum = 0.0;
    let r = i32(u.radius);
    for (var i: i32 = -r; i <= r; i = i + 1) {
        let offset = u.direction * f32(i);
        let w = exp(-f32(i * i) / two_sigma_sq);
        let sample = textureSample(src, src_sampler, in.uv + offset);
        accum = accum + sample * w;
        weight_sum = weight_sum + w;
    }
    return accum / max(weight_sum, 1e-6);
}
