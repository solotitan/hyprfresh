use super::Screensaver;
use std::collections::HashMap;

/// Blank screensaver - just a black screen
/// Useful for OLED burn-in prevention or minimal power usage
pub struct Blank;

impl Blank {
    pub fn new() -> Self {
        Self
    }
}

impl Screensaver for Blank {
    fn name(&self) -> &str {
        "blank"
    }

    fn description(&self) -> &str {
        "Black screen (DPMS-like, minimal power)"
    }

    fn init(&mut self, _width: u32, _height: u32, _options: &HashMap<String, toml::Value>) {
        // Nothing to initialize
    }

    fn update(&mut self, _dt: f32) {
        // Nothing to update
    }

    fn fragment_shader(&self) -> &str {
        r#"
@fragment
fn fs_main(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, 1.0);
}
"#
    }
}
