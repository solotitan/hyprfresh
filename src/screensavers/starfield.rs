use super::Screensaver;
use std::collections::HashMap;

/// Classic starfield fly-through screensaver
pub struct Starfield {
    time: f32,
    speed: f32,
    star_count: f32,
}

impl Starfield {
    pub fn new() -> Self {
        Self {
            time: 0.0,
            speed: 1.0,
            star_count: 200.0,
        }
    }
}

impl Screensaver for Starfield {
    fn name(&self) -> &str {
        "starfield"
    }

    fn description(&self) -> &str {
        "Classic starfield fly-through"
    }

    fn init(&mut self, _width: u32, _height: u32, options: &HashMap<String, toml::Value>) {
        if let Some(toml::Value::Float(s)) = options.get("speed") {
            self.speed = *s as f32;
        }
        if let Some(toml::Value::Integer(n)) = options.get("stars") {
            self.star_count = *n as f32;
        }
    }

    fn update(&mut self, dt: f32) {
        self.time += dt * self.speed;
    }

    fn fragment_shader(&self) -> &str {
        r#"
struct Uniforms {
    time: f32,
    resolution: vec2f,
    speed: f32,
    star_count: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

fn hash21(p: vec2f) -> f32 {
    var p3 = fract(vec3f(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3f(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fs_main(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let uv = (pos.xy - u.resolution * 0.5) / min(u.resolution.x, u.resolution.y);
    var color = vec3f(0.0);

    // Multiple layers for depth
    for (var layer = 0.0; layer < 4.0; layer += 1.0) {
        let depth = fract(layer * 0.25 + u.time * u.speed * 0.1);
        let scale = mix(20.0, 0.5, depth);
        let fade = depth * depth;

        let grid_uv = uv * scale + vec2f(layer * 17.3, layer * 31.7);
        let grid_id = floor(grid_uv);
        let grid_fract = fract(grid_uv) - 0.5;

        let rnd = hash21(grid_id);

        // Star size decreases with depth
        let star_size = (1.0 - depth) * 0.03;
        let d = length(grid_fract - vec2f(rnd - 0.5, fract(rnd * 34.56) - 0.5));
        let star = smoothstep(star_size, 0.0, d);

        // Slight color variation per star
        let star_color = vec3f(
            0.8 + 0.2 * fract(rnd * 123.45),
            0.8 + 0.2 * fract(rnd * 234.56),
            0.9 + 0.1 * fract(rnd * 345.67),
        );

        color += star * star_color * fade;
    }

    return vec4f(color, 1.0);
}
"#
    }
}
