struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Uniforms {
    rect: vec4<f32>,
    // Corner radii (top left, top right, bottom right, bottom left) of the
    // drawn quad, in the same units as rect.
    radius: vec4<f32>,
    // xy: size of the drawn quad, so each fragment can work out its pixel
    // position from its uv for the rounded corner clip. zw is unused.
    size: vec4<f32>,
}

@group(0) @binding(0)
var tex_y: texture_2d<f32>;

@group(0) @binding(1)
var tex_uv: texture_2d<f32>;

@group(0) @binding(2)
var s: sampler;

@group(0) @binding(3)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var quad = array<vec4<f32>, 6>(
        vec4<f32>(uniforms.rect.xy, 0.0, 0.0),
        vec4<f32>(uniforms.rect.zy, 1.0, 0.0),
        vec4<f32>(uniforms.rect.xw, 0.0, 1.0),
        vec4<f32>(uniforms.rect.zy, 1.0, 0.0),
        vec4<f32>(uniforms.rect.zw, 1.0, 1.0),
        vec4<f32>(uniforms.rect.xw, 0.0, 1.0),
    );

    var out: VertexOutput;
    out.uv = quad[in_vertex_index].zw;
    out.position = vec4<f32>(quad[in_vertex_index].xy, 1.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // BT.709 precomputed coefficents
    let yuv2rgb = mat3x3<f32>(
        1, 0, 1.5748,
        1, -0.1873, -0.4681,
        1, 1.8556, 0,
    );

    var yuv = vec3<f32>(0.0);
    yuv.x = (textureSample(tex_y, s, in.uv).r - 0.0625) / 0.8588;
    yuv.y = (textureSample(tex_uv, s, in.uv).r - 0.5) / 0.8784;
    yuv.z = (textureSample(tex_uv, s, in.uv).g - 0.5) / 0.8784;

    var rgb = clamp(yuv * yuv2rgb, vec3<f32>(0), vec3<f32>(1));

    // Rounded corner clip:
    // signed distance of the fragment from the
    // rounded rectangle, in pixel units, with the radius of the corner the
    // fragment is in. d < 0 is inside, d > 0 outside.
    let p = abs((in.uv - vec2<f32>(0.5)) * uniforms.size.xy);
    let half_size = uniforms.size.xy / vec2<f32>(2.0);
    var r = uniform_radius(uniforms.radius, in.uv);
    let q = p - (half_size - vec2<f32>(r));
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;

    // One pixel feather on each side of the outline for antialiasing.
    let alpha = 1.0 - smoothstep(0.0, 1.0, d);

    return vec4<f32>(rgb * alpha, alpha);
}

fn uniform_radius(radius: vec4<f32>, uv: vec2<f32>) -> f32 {
    // Order: top left, top right, bottom right, bottom left, y grows down.
    if (uv.x < 0.5) {
        if (uv.y < 0.5) {
            return radius.x;
        } else {
            return radius.w;
        }
    } else {
        if (uv.y < 0.5) {
            return radius.y;
        } else {
            return radius.z;
        }
    }
}
