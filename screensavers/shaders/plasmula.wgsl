// Plasmula — Dracula-themed plasma waves
//
// Layered sine-wave plasma effect using the Dracula color palette.
// Smooth, hypnotic color blending across the screen.

// Dracula palette
const BG:      vec3<f32> = vec3<f32>(0.157, 0.165, 0.212);  // #282a36
const PURPLE:  vec3<f32> = vec3<f32>(0.741, 0.576, 0.976);  // #bd93f9
const PINK:    vec3<f32> = vec3<f32>(1.000, 0.475, 0.776);  // #ff79c6
const CYAN:    vec3<f32> = vec3<f32>(0.545, 0.914, 0.992);  // #8be9fd
const GREEN:   vec3<f32> = vec3<f32>(0.314, 0.980, 0.482);  // #50fa7b
const ORANGE:  vec3<f32> = vec3<f32>(1.000, 0.722, 0.424);  // #ffb86c
const RED:     vec3<f32> = vec3<f32>(1.000, 0.333, 0.333);  // #ff5555

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time * 0.4;
    let aspect = u.resolution.x / u.resolution.y;
    let p = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5) * 4.0;

    // Layer 1: large slow waves
    let v1 = sin(p.x * 1.2 + t * 0.7) + sin(p.y * 1.1 - t * 0.5);

    // Layer 2: diagonal ripple
    let v2 = sin((p.x + p.y) * 0.9 + t * 0.6) + sin(length(p) * 1.5 - t * 0.8);

    // Layer 3: radial pulse
    let d = length(p - vec2<f32>(sin(t * 0.3) * 1.5, cos(t * 0.4) * 1.5));
    let v3 = sin(d * 2.5 - t * 1.2);

    // Layer 4: fine detail
    let v4 = sin(p.x * 3.0 - t * 0.9) * sin(p.y * 2.5 + t * 0.7);

    // Combine layers into a smooth value [-1, 1] range
    let plasma = (v1 + v2 + v3 + v4) * 0.25;

    // Map plasma value to Dracula colors using smooth blending
    // Divide the [-1, 1] range into color zones
    let n = plasma * 0.5 + 0.5; // normalize to [0, 1]

    // 5-color gradient: purple -> pink -> cyan -> green -> orange
    var color: vec3<f32>;
    if n < 0.2 {
        color = mix(PURPLE, PINK, n * 5.0);
    } else if n < 0.4 {
        color = mix(PINK, CYAN, (n - 0.2) * 5.0);
    } else if n < 0.6 {
        color = mix(CYAN, GREEN, (n - 0.4) * 5.0);
    } else if n < 0.8 {
        color = mix(GREEN, ORANGE, (n - 0.6) * 5.0);
    } else {
        color = mix(ORANGE, PURPLE, (n - 0.8) * 5.0);
    }

    // Blend toward dark background at edges for depth
    let vignette = 1.0 - length(uv - 0.5) * 0.6;
    color = mix(BG, color, vignette * 0.85 + 0.15);

    return vec4<f32>(color, 1.0);
}
