struct TileUniforms {
    origin: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
    border_width: f32,
    resolution: vec2<f32>,
    border_color: vec4<f32>,
    tint_color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> u: TileUniforms;
@group(0) @binding(1)
var bg_texture: texture_2d<f32>;
@group(0) @binding(2)
var bg_sampler: sampler;

// Fullscreen triangle covering clip space; no vertex buffer needed. Iced
// restricts the render pass's viewport/scissor to this primitive's own
// bounds, so only the pixels inside this tile actually get rasterized.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// Signed distance to a rounded box (Inigo Quilez's formulation); negative inside.
fn rounded_box_sdf(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(point) - half_size + vec2<f32>(radius, radius);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let half_size = u.size * 0.5;
    let local = frag_coord.xy - u.origin - half_size;
    let dist = rounded_box_sdf(local, half_size, u.radius);

    let aa = max(fwidth(dist) * 0.5, 0.0001);
    let shape_alpha = 1.0 - smoothstep(-aa, aa, dist);
    if shape_alpha <= 0.0 {
        discard;
    }

    // `bg_texture` holds a tiny, low-resolution render of the app's animated
    // background, using the same window-normalized UVs as the full-size one.
    // Sampling it at this pixel's absolute window position - bilinearly
    // upscaled - is what fakes the blur.
    let uv = frag_coord.xy / u.resolution;
    let bg_sample = textureSample(bg_texture, bg_sampler, uv);

    let tinted = mix(bg_sample.rgb, u.tint_color.rgb, u.tint_color.a);
    let border_t = smoothstep(-u.border_width - aa, -u.border_width + aa, dist);
    let color = mix(tinted, u.border_color.rgb, border_t);

    return vec4<f32>(color, shape_alpha);
}
