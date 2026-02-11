//! Screensaver modules
//!
//! Each screensaver implements the `Screensaver` trait, providing
//! initialization, per-frame updates, and a fragment shader or
//! draw commands for rendering.

pub mod blank;
pub mod matrix;
pub mod starfield;

use std::collections::HashMap;

/// All built-in screensaver names
pub const AVAILABLE: &[(&str, &str)] = &[
    ("blank", "Black screen (DPMS-like, minimal power)"),
    ("matrix", "Matrix digital rain effect"),
    ("starfield", "Classic starfield fly-through"),
];

/// Check if a screensaver name is valid
pub fn is_valid(name: &str) -> bool {
    AVAILABLE.iter().any(|(n, _)| *n == name)
}

/// Trait that all screensaver modules must implement
#[allow(dead_code)]
pub trait Screensaver {
    /// Human-readable name
    fn name(&self) -> &str;

    /// Short description
    fn description(&self) -> &str;

    /// Initialize the screensaver with the given viewport dimensions
    fn init(&mut self, width: u32, height: u32, options: &HashMap<String, toml::Value>);

    /// Update state for the next frame (dt = seconds since last frame)
    fn update(&mut self, dt: f32);

    /// Return the WGSL fragment shader source for this screensaver
    /// The shader receives: time (f32), resolution (vec2f), and any uniforms
    fn fragment_shader(&self) -> &str;
}

/// List all available screensavers
pub fn list_available() {
    println!("Available screensavers:");
    println!();
    for (name, desc) in AVAILABLE {
        println!("  {:<16} {}", name, desc);
    }
    println!();
    println!("Set the screensaver in ~/.config/hypr/hyprfresh.toml:");
    println!("  [screensaver]");
    println!("  name = \"matrix\"");
}

/// Get a screensaver instance by name
#[allow(dead_code)]
pub fn get(name: &str) -> Option<Box<dyn Screensaver>> {
    match name {
        "blank" => Some(Box::new(blank::Blank::new())),
        "matrix" => Some(Box::new(matrix::Matrix::new())),
        "starfield" => Some(Box::new(starfield::Starfield::new())),
        _ => None,
    }
}
