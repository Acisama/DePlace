struct BlurUniforms {
    texel_size: vec2<f32>,
    direction: vec2<f32>,
    radius: f32,
    _pad: f32,
}

@group(0) @binding(0)
var<uniform> u: BlurUniforms;
@group(0) @binding(1)
var src_texture: texture_2d<f32>;
@group(0) @binding(2)
var src_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

const HALF_TAP_COUNT: i32 = 12;

// One direction of a separable Gaussian blur. Run once with `direction =
// (1, 0)` and once with `direction = (0, 1)` (reading the first pass's
// output) to get a full 2D blur at roughly half the cost of a single-pass
// NxN kernel.
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag_coord.xy * u.texel_size;
    let sigma = max(u.radius * 0.5, 0.5);

    var color = vec4<f32>(0.0);
    var total_weight = 0.0;
    for (var i = -HALF_TAP_COUNT; i <= HALF_TAP_COUNT; i = i + 1) {
        let offset_texels = f32(i) / f32(HALF_TAP_COUNT) * u.radius;
        let weight = exp(-(offset_texels * offset_texels) / (2.0 * sigma * sigma));
        let sample_uv = uv + u.direction * offset_texels * u.texel_size;
        color += textureSample(src_texture, src_sampler, sample_uv) * weight;
        total_weight += weight;
    }

    return color / total_weight;
}
