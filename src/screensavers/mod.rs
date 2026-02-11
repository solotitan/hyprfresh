//! Screensaver modules
//!
//! Each screensaver implements the `Screensaver` trait, providing
//! initialization, per-frame updates, and a fragment shader or
//! draw commands for rendering.
//!
//! Custom shaders can be placed in `~/.config/hypr/hyprfresh/shaders/`.
//! Any `.wgsl` file in that directory becomes available as a screensaver
//! using the filename (without extension) as the name.

pub mod blank;
pub mod matrix;
pub mod starfield;

use log::debug;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// All built-in screensaver names
pub const BUILTIN: &[(&str, &str)] = &[
    ("blank", "Black screen (DPMS-like, minimal power)"),
    ("matrix", "Matrix digital rain effect"),
    ("starfield", "Classic starfield fly-through"),
];

/// Default directory for custom shaders
const CUSTOM_SHADER_DIR: &str = "~/.config/hypr/hyprfresh/shaders";

/// Resolve the custom shader directory path
pub fn custom_shader_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(format!("{}/.config/hypr/hyprfresh/shaders", home));
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

/// Discover custom shaders from the filesystem
pub fn discover_custom() -> Vec<(String, PathBuf)> {
    let Some(dir) = custom_shader_dir() else {
        return Vec::new();
    };

    let mut custom = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wgsl")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            let name = stem.to_string();
            debug!("Found custom shader: {} ({})", name, path.display());
            custom.push((name, path));
        }
    }

    custom.sort_by(|a, b| a.0.cmp(&b.0));
    custom
}

/// Load a custom shader's fragment source from disk
pub fn load_custom_shader(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    // Basic validation: must contain fs_main entry point
    if !source.contains("fn fs_main") {
        return Err(format!(
            "Shader {} missing required 'fn fs_main' entry point",
            path.display()
        ));
    }

    Ok(source)
}

/// Check if a screensaver name is valid (built-in or valid custom shader)
pub fn is_valid(name: &str) -> bool {
    if BUILTIN.iter().any(|(n, _)| *n == name) {
        return true;
    }
    // Check for custom shader file AND validate it has fs_main
    if let Some(dir) = custom_shader_dir() {
        let path = dir.join(format!("{}.wgsl", name));
        if path.is_file() {
            return load_custom_shader(&path).is_ok();
        }
    }
    false
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

/// List all available screensavers (built-in + custom)
pub fn list_available() {
    println!("Available screensavers:");
    println!();

    // Built-in
    for (name, desc) in BUILTIN {
        println!("  {:<16} {}", name, desc);
    }

    // Custom
    let custom = discover_custom();
    if !custom.is_empty() {
        println!();
        println!("Custom shaders ({}):", CUSTOM_SHADER_DIR);
        println!();
        for (name, path) in &custom {
            let overrides = if BUILTIN.iter().any(|(n, _)| *n == name) {
                " (overrides built-in)"
            } else {
                ""
            };
            println!(
                "  {:<16} {}{}",
                name,
                path.file_name().unwrap_or_default().to_string_lossy(),
                overrides
            );
        }
    }

    println!();
    println!("Set the screensaver in ~/.config/hypr/hyprfresh.toml:");
    println!("  [screensaver]");
    println!("  name = \"matrix\"");
    println!();
    println!("Custom shaders: place .wgsl files in {}", CUSTOM_SHADER_DIR);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names_are_valid() {
        assert!(is_valid("blank"));
        assert!(is_valid("matrix"));
        assert!(is_valid("starfield"));
    }

    #[test]
    fn unknown_name_is_invalid() {
        assert!(!is_valid("nonexistent_shader_xyz"));
    }

    #[test]
    fn load_custom_shader_validates_entry_point() {
        let dir = std::env::temp_dir().join("hyprfresh_test_shaders");
        let _ = std::fs::create_dir_all(&dir);

        // Valid shader
        let valid = dir.join("test_valid.wgsl");
        std::fs::write(
            &valid,
            "@fragment\nfn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n",
        ).unwrap();
        assert!(load_custom_shader(&valid).is_ok());

        // Invalid shader (no fs_main)
        let invalid = dir.join("test_invalid.wgsl");
        std::fs::write(&invalid, "fn some_other_function() -> f32 { return 1.0; }\n").unwrap();
        assert!(load_custom_shader(&invalid).is_err());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
