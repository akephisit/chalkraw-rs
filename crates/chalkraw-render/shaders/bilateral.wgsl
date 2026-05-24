// Bilateral filter — edge-preserving smoothing for Noise Reduction.
// Single 2D pass (not separable). Each output pixel is a weighted average
// of a (2r+1)×(2r+1) neighbourhood where weights combine:
//   spatial weight: Gaussian on pixel distance (sigma_spatial)
//   range  weight: Gaussian on RGB colour difference (sigma_range)
// Edges → large colour difference → low range weight → edge preserved.
// Flat regions → small colour difference → high range weight → smoothed.

// BilateralUniforms: 8 floats = 32 bytes, all f32 to avoid WGSL vec3 alignment surprises.
// Mirrors the Rust BilateralUniforms struct field-for-field (both 32 bytes).
struct BilateralUniforms {
    dir_x:         f32,   // offset  0 — unused (direction.x kept for layout parity)
    dir_y:         f32,   // offset  4 — unused (direction.y)
    sigma_spatial: f32,   // offset  8 — spatial Gaussian sigma in pixels
    sigma_range:   f32,   // offset 12 — range Gaussian sigma in linear RGB units
    radius:        f32,   // offset 16 — half-window size (3 = 7×7)
    _pad0:         f32,   // offset 20 — padding
    _pad1:         f32,   // offset 24 — padding
    _pad2:         f32,   // offset 28 — padding
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: BilateralUniforms;

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
    let center = textureSample(src, src_sampler, in.uv).rgb;
    let dims = vec2<f32>(textureDimensions(src, 0));
    let texel = vec2<f32>(1.0 / dims.x, 1.0 / dims.y);
    let sigma_s = max(u.sigma_spatial, 0.001);
    let sigma_r = max(u.sigma_range,   0.001);
    let sigma_s_sq = 2.0 * sigma_s * sigma_s;
    let sigma_r_sq = 2.0 * sigma_r * sigma_r;
    let r = i32(u.radius);

    var accum = vec3<f32>(0.0);
    var weight_sum = 0.0;
    for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
            let offset = vec2<f32>(f32(dx), f32(dy)) * texel;
            let sample_rgb = textureSample(src, src_sampler, in.uv + offset).rgb;
            let spatial_w = exp(-f32(dx * dx + dy * dy) / sigma_s_sq);
            let diff = sample_rgb - center;
            let range_w = exp(-(diff.x * diff.x + diff.y * diff.y + diff.z * diff.z) / sigma_r_sq);
            let w = spatial_w * range_w;
            accum = accum + sample_rgb * w;
            weight_sum = weight_sum + w;
        }
    }
    return vec4<f32>(accum / max(weight_sum, 1e-6), 1.0);
}
