use super::Screensaver;
use std::collections::HashMap;

/// Matrix digital rain screensaver
#[allow(dead_code)]
pub struct Matrix {
    time: f32,
    color_r: f32,
    color_g: f32,
    color_b: f32,
    speed: f32,
    density: f32,
}

impl Matrix {
    pub fn new() -> Self {
        Self {
            time: 0.0,
            color_r: 0.0,
            color_g: 1.0,
            color_b: 0.0,
            speed: 1.0,
            density: 1.0,
        }
    }
}

impl Screensaver for Matrix {
    fn name(&self) -> &str {
        "matrix"
    }

    fn description(&self) -> &str {
        "Matrix digital rain effect"
    }

    fn init(&mut self, _width: u32, _height: u32, options: &HashMap<String, toml::Value>) {
        if let Some(toml::Value::Float(s)) = options.get("speed") {
            self.speed = *s as f32;
        }
        if let Some(toml::Value::Float(d)) = options.get("density") {
            self.density = *d as f32;
        }
        // Allow custom color via [screensaver.options] color = [r, g, b]
        if let Some(toml::Value::Array(c)) = options.get("color")
            && c.len() >= 3
        {
            self.color_r = c[0].as_float().unwrap_or(0.0) as f32;
            self.color_g = c[1].as_float().unwrap_or(1.0) as f32;
            self.color_b = c[2].as_float().unwrap_or(0.0) as f32;
        }
    }

    fn update(&mut self, dt: f32) {
        self.time += dt * self.speed;
    }

    fn fragment_shader(&self) -> &str {
        // Matrix rain implemented as a WGSL fragment shader
        // Uses procedural generation - no texture atlas needed
        r#"
struct Uniforms {
    time: f32,
    resolution: vec2f,
    color: vec3f,
    speed: f32,
    density: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// Pseudo-random hash
fn hash(p: vec2f) -> f32 {
    let h = dot(p, vec2f(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

@fragment
fn fs_main(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let uv = pos.xy / u.resolution;
    let col_width = 20.0 * u.density;
    let col = floor(uv.x * col_width);

    // Each column has a random speed and offset
    let col_seed = hash(vec2f(col, 0.0));
    let col_speed = 0.5 + col_seed * 1.5;
    let col_offset = hash(vec2f(col, 1.0)) * 100.0;

    // Scrolling position
    let scroll = u.time * col_speed * u.speed + col_offset;
    let char_y = floor((1.0 - uv.y) * 40.0 + scroll);

    // Character brightness (head is bright, tail fades)
    let head = fract(scroll);
    let dist_from_head = fract((1.0 - uv.y) * 40.0 + scroll) ;
    let brightness = smoothstep(1.0, 0.0, dist_from_head) * 0.8;

    // Random character flicker
    let char_hash = hash(vec2f(col, char_y));
    let flicker = step(0.3, char_hash);

    let intensity = brightness * flicker;

    // Green tint with slight color variation
    let color = u.color * intensity;

    return vec4f(color, 1.0);
}
"#
    }
}
