// Plasmula — Dark plasma waves
//
// Layered sine-wave plasma effect with a dark palette.
// Primary: #6000FF (electric purple), #00FF6C (neon green)
// Accents: deep teal, warm amber — no whites, dark grays only.

// Palette
const BG:      vec3<f32> = vec3<f32>(0.040, 0.040, 0.055);  // #0a0a0e — near-black
const PURPLE:  vec3<f32> = vec3<f32>(0.376, 0.000, 1.000);  // #6000FF
const GREEN:   vec3<f32> = vec3<f32>(0.000, 1.000, 0.424);  // #00FF6C
const TEAL:    vec3<f32> = vec3<f32>(0.000, 0.400, 0.380);  // #006661 — deep teal
const AMBER:   vec3<f32> = vec3<f32>(0.600, 0.340, 0.000);  // #995700 — warm amber
const GRAY:    vec3<f32> = vec3<f32>(0.120, 0.120, 0.140);  // #1e1e24 — dark gray

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time * 0.35;
    let aspect = u.resolution.x / u.resolution.y;
    let p = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5) * 4.0;

    // Layer 1: large slow waves
    let v1 = sin(p.x * 1.2 + t * 0.7) + sin(p.y * 1.1 - t * 0.5);

    // Layer 2: diagonal ripple
    let v2 = sin((p.x + p.y) * 0.9 + t * 0.6) + sin(length(p) * 1.5 - t * 0.8);

    // Layer 3: radial pulse from a drifting center
    let d = length(p - vec2<f32>(sin(t * 0.3) * 1.5, cos(t * 0.4) * 1.5));
    let v3 = sin(d * 2.5 - t * 1.2);

    // Layer 4: fine detail
    let v4 = sin(p.x * 3.0 - t * 0.9) * sin(p.y * 2.5 + t * 0.7);

    // Combine layers — smooth value in [-1, 1]
    let plasma = (v1 + v2 + v3 + v4) * 0.25;

    // Normalize to [0, 1]
    let n = plasma * 0.5 + 0.5;

    // 5-zone gradient: purple -> teal -> green -> amber -> purple (loop)
    var color: vec3<f32>;
    if n < 0.2 {
        color = mix(PURPLE, TEAL, n * 5.0);
    } else if n < 0.4 {
        color = mix(TEAL, GREEN, (n - 0.2) * 5.0);
    } else if n < 0.6 {
        color = mix(GREEN, AMBER, (n - 0.4) * 5.0);
    } else if n < 0.8 {
        color = mix(AMBER, GRAY, (n - 0.6) * 5.0);
    } else {
        color = mix(GRAY, PURPLE, (n - 0.8) * 5.0);
    }

    // Darken overall — keep colors subdued, never bright white
    color = color * 0.55;

    // Vignette: fade to near-black at edges
    let vignette = 1.0 - length(uv - 0.5) * 0.8;
    color = mix(BG, color, clamp(vignette, 0.0, 1.0));

    return vec4<f32>(color, 1.0);
}
