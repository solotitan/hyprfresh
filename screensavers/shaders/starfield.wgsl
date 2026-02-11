// Classic starfield fly-through effect
//
// Stars appear to fly toward the viewer from a central vanishing point.

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(
        dot(p, vec2<f32>(127.1, 311.7)),
        dot(p, vec2<f32>(269.5, 183.3))
    );
    return fract(sin(q) * 43758.5453);
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;

    // Center coordinates with aspect correction
    var p = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    var color = vec3<f32>(0.0);

    // Multiple star layers at different depths
    for (var layer = 0; layer < 4; layer++) {
        let depth = f32(layer + 1) * 0.25;
        let speed = 0.1 + depth * 0.3;

        // Tile space for star placement
        let scale = 10.0 + f32(layer) * 8.0;
        let st = p * scale;
        let cell = floor(st);
        let cell_uv = fract(st) - 0.5;

        // Star position within cell (random offset)
        let star_pos = hash2(cell + f32(layer) * 100.0) - 0.5;
        let d = length(cell_uv - star_pos * 0.8);

        // Star size varies with time (simulates z-motion)
        let z = fract(t * speed + hash2(cell).x);
        let size = mix(0.001, 0.04, z * z);

        // Brightness: brighter as stars get "closer"
        let brightness = smoothstep(size, size * 0.3, d) * z * z;

        // Slight color variation
        let star_color = mix(
            vec3<f32>(0.8, 0.9, 1.0),
            vec3<f32>(1.0, 0.95, 0.8),
            hash2(cell + 50.0).x
        );

        color += star_color * brightness * depth;
    }

    // Subtle vignette
    let vignette = 1.0 - length(uv - 0.5) * 0.8;
    color *= vignette;

    return vec4<f32>(color, 1.0);
}
