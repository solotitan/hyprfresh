use super::Screensaver;
use std::collections::HashMap;

/// Plasmula screensaver - Dracula-themed plasma waves
#[allow(dead_code)]
pub struct Plasmula;

impl Plasmula {
    pub fn new() -> Self {
        Self
    }
}

impl Screensaver for Plasmula {
    fn name(&self) -> &str {
        "plasmula"
    }

    fn description(&self) -> &str {
        "Dracula-themed plasma waves"
    }

    fn init(&mut self, _width: u32, _height: u32, _options: &HashMap<String, toml::Value>) {}

    fn update(&mut self, _dt: f32) {}

    fn fragment_shader(&self) -> &str {
        include_str!("../../screensavers/shaders/plasmula.wgsl")
    }
}
