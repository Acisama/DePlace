struct Uniforms {
    time: f32,
    last_changed_time: f32,
    state: f32,
    prev_state: f32,
    resolution: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> u: Uniforms;

const k_loading_speed_mult: f32 = 15.0;
const k_normal_speed_mult: f32 = 1.0;
const k_state_transition_seconds: f32 = 0.5;
const k_state_count: f32 = 4.0;
const k_state_max: f32 = k_state_count - 1.0;
const k_speed_drop_pow: f32 = 2.0;
const k_speed_state_pow: f32 = 0.2;
const k_loading_radius_mult: f32 = 0.7;
const k_normal_radius_mult: f32 = 1.08;

const k_bg_wave_freq: f32 = 24.0;
const k_bg_diag_threshold: f32 = 0.4;

const k_hex_ring_radius: f32 = 0.15;
const k_hex_ring_width: f32 = 0.02;

const k_core_circle_radius: f32 = 0.06;
const k_core_circle_width: f32 = 0.01;

const k_orbit_outer_base: f32 = 0.25;
const k_orbit_outer_step: f32 = 0.12;
const k_orbit_inner_base: f32 = 0.08;
const k_orbit_inner_step: f32 = 0.05;
const k_orbit_ring_width_base: f32 = 0.003;
const k_orbit_ring_width_step: f32 = 0.001;
const k_orbit_outer_spread: f32 = 1.12;
const k_orbit_outer_spacing_pow: f32 = 1.4;
const k_orbit_count: f32 = 4.0;
const k_orbit_extra_count: f32 = 2.0;
const k_orbit_extra_distance_mult: f32 = 1.4;
const k_orbit_extra_inset: f32 = 0.25;
const k_orbit_total_count: f32 = k_orbit_count + k_orbit_extra_count;

const k_orbit_speed_mult_base: f32 = 0.85;
const k_orbit_speed_mult_step: f32 = 0.12;

const k_dash_threshold: f32 = 0.1;
const k_head_threshold: f32 = 0.9;

// Fullscreen triangle covering clip space; no vertex buffer needed.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn aastep(threshold: f32, value: f32) -> f32 {
    let w = fwidth(value) * 0.5;
    return smoothstep(threshold - w, threshold + w, value);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    var uv = frag_coord.xy / u.resolution;
    // WGSL's @builtin(position) has a top-left origin (y grows down), unlike
    // GLSL's gl_FragCoord (bottom-left origin); flip to match the original look.
    uv.y = 1.0 - uv.y;
    uv = uv * 2.0 - vec2<f32>(1.0, 1.0);
    uv.x *= u.resolution.x / u.resolution.y;

    // --- 1. Background ---
    var bg_color = vec3<f32>(0.0, 0.0, 0.0);

    let bg_wave = sin((uv.x + uv.y) * k_bg_wave_freq + u.time * 0.5);
    let bg_diag = aastep(k_bg_diag_threshold, bg_wave);
    bg_color += vec3<f32>(0.02, 0.03, 0.08) * bg_diag;

    // --- Distance Metric ---
    // 0.866025 is approx sqrt(3)/2, creating a pointy-topped hexagon.
    let p = abs(uv);
    let hex_dist = max(p.x, p.x * 0.5 + p.y * 0.866025);
    let circle_dist = length(uv);

    // --- 2. Glowing Center (Circular) ---
    let core_glow = exp(-circle_dist * 5.0);
    let core_ring = (1.0 - aastep(k_hex_ring_width, abs(hex_dist - k_hex_ring_radius))) * 0.5;
    let core_circle = 1.0 - aastep(k_core_circle_width, abs(circle_dist - k_core_circle_radius));
    var center_color = vec3<f32>(0.06, 0.25, 0.4) * (core_glow + core_ring);
    center_color += vec3<f32>(0.08, 0.3, 0.5) * core_circle;

    // --- 3. Circling Lines (Orbitals) ---
    let angle = atan2(uv.y, uv.x);
    var orbital_color = vec3<f32>(0.0, 0.0, 0.0);
    let state_index = clamp(u.state, 0.0, k_state_max);
    let prev_state = clamp(u.prev_state, 0.0, k_state_max);
    let transition_seconds = max(k_state_transition_seconds, 0.0001);
    let state_elapsed = u.time - u.last_changed_time;
    var state_t: f32 = 1.0;
    if u.last_changed_time != 0.0 {
        state_t = clamp(state_elapsed / transition_seconds, 0.0, 1.0);
    }
    let state_ease = state_t * state_t * (3.0 - 2.0 * state_t);
    let state_mix = mix(prev_state, state_index, state_ease);
    let distance_factor = state_mix / k_state_max;

    let prev_distance = prev_state / k_state_max;
    let curr_distance = state_index / k_state_max;
    let speed_prev = mix(k_loading_speed_mult, k_normal_speed_mult, pow(prev_distance, k_speed_state_pow));
    let speed_curr = mix(k_loading_speed_mult, k_normal_speed_mult, pow(curr_distance, k_speed_state_pow));

    var phase_time = u.time * speed_curr;
    if u.last_changed_time != 0.0 {
        let t0 = u.last_changed_time;
        let s = state_t;
        var speed_e_int = s * s * s - 0.5 * s * s * s * s;

        if speed_curr < speed_prev {
            let pw = k_speed_drop_pow;
            speed_e_int = s - (1.0 - pow(1.0 - s, pw + 1.0)) / (pw + 1.0);
        }

        let transition_integral = transition_seconds *
                (speed_prev * s + (speed_curr - speed_prev) * speed_e_int);
        let after = max(u.time - t0 - transition_seconds, 0.0);
        phase_time = speed_prev * t0 + transition_integral + speed_curr * after;
    }

    let outer_spread_base = k_orbit_outer_base + k_orbit_count * k_orbit_outer_step * (1.0 - k_orbit_outer_spread);
    let outer_min = (outer_spread_base + 1.0 * k_orbit_outer_step * k_orbit_outer_spread)
            * k_normal_radius_mult;
    let outer_max = (outer_spread_base + k_orbit_count * k_orbit_outer_step * k_orbit_outer_spread)
            * k_normal_radius_mult;

    for (var i: f32 = 1.0; i <= k_orbit_total_count; i = i + 1.0) {
        let t = clamp((i - 1.0) / (k_orbit_count - 1.0), 0.0, 1.0);
        let outer_r = mix(outer_min, outer_max, pow(t, k_orbit_outer_spacing_pow));
        let inner_r = (k_orbit_inner_base + i * k_orbit_inner_step) * k_loading_radius_mult;
        let base_r = mix(inner_r, outer_r, distance_factor);

        let extra_mask = step(k_orbit_count + 0.5, i);
        let extra_index = i - k_orbit_count;
        let extra_step = (k_orbit_extra_distance_mult - 1.0) * outer_max;
        let extra_target = outer_max + extra_index * extra_step;
        let extra_r = extra_target + (1.0 - distance_factor) * k_orbit_extra_inset;
        let r = mix(base_r, extra_r, extra_mask);

        // `i` is always a positive integer-valued float here, so `%` (WGSL's
        // truncated remainder) agrees with GLSL's `mod` (floored remainder).
        var base_speed = -1.0 * (0.3 + i * 0.15);
        if i % 2.0 == 0.0 {
            base_speed = 1.0 * (0.3 + i * 0.15);
        }
        let speed_mult = k_orbit_speed_mult_base + k_orbit_speed_mult_step * i;
        let segments = 2.0 + i;
        let current_angle = angle * segments + phase_time * base_speed * speed_mult;

        let ring_width = k_orbit_ring_width_base + k_orbit_ring_width_step * i;

        let ring_mask = 1.0 - aastep(ring_width, abs(hex_dist - r));
        let dash = aastep(k_dash_threshold, sin(current_angle));
        let head = aastep(k_head_threshold, fract(current_angle / 6.28318));

        let ring_alpha = mix(1.0, distance_factor, extra_mask);
        let col = 0.5 + 0.5 * cos(u.time * 0.4 + i + vec3<f32>(0.0, 1.5, 3.0));
        orbital_color += ring_mask * dash * col * (0.8 + head * 2.5) * ring_alpha;
    }

    let final_color = vec3<f32>(0.03, 0.03, 0.05) + bg_color + center_color + orbital_color;

    return vec4<f32>(final_color, 1.0);
}
