// Matrix digital rain effect
//
// Simulates falling green characters using procedural noise.
// Each column falls at a different speed with varying brightness.

// Hash function for pseudo-random values
fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let res = u.resolution;
    let t = u.time;

    // Grid: divide screen into character cells
    let cell_size = vec2<f32>(12.0, 16.0);
    let grid = floor(uv * res / cell_size);
    let cell_uv = fract(uv * res / cell_size);

    // Per-column properties
    let col = grid.x;
    let col_speed = 0.5 + hash(vec2<f32>(col, 0.0)) * 2.0;
    let col_offset = hash(vec2<f32>(col, 1.0)) * 100.0;

    // Falling position
    let fall = grid.y + t * col_speed * 8.0 + col_offset;

    // Character: pseudo-random per cell, changes over time
    let char_hash = hash(vec2<f32>(col, floor(fall)));

    // Brightness: brighter at the leading edge, fading trail
    let trail_len = 8.0 + hash(vec2<f32>(col, 2.0)) * 16.0;
    let head = fract(t * col_speed * 0.5 + col_offset * 0.01);
    let row_norm = 1.0 - uv.y;
    let dist = fract(head - row_norm + 1.0);
    let brightness = smoothstep(0.0, 1.0 / trail_len, 1.0 - dist) * step(dist, 1.0 / trail_len * trail_len);

    // Character glyph: simple block pattern
    let glyph = step(0.15, cell_uv.x) * step(cell_uv.x, 0.85) *
                step(0.1, cell_uv.y) * step(cell_uv.y, 0.9);
    let pattern = step(0.3, char_hash) * glyph;

    // Color: green with slight variation
    let green = vec3<f32>(0.0, brightness * pattern, 0.0);

    // Head glow: white-green at the leading edge
    let head_glow = smoothstep(0.02, 0.0, dist) * pattern * 0.5;
    let color = green + vec3<f32>(head_glow, head_glow, head_glow);

    return vec4<f32>(color, 1.0);
}
