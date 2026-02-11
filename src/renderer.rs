//! Wayland surface renderer
//!
//! Creates wlr-layer-shell surfaces on specific outputs and renders
//! screensaver animations using wgpu.
//!
//! Architecture:
//! - One layer surface per monitor that needs a screensaver
//! - Surfaces are created at the overlay layer (above everything)
//! - Each surface gets its own wgpu render pipeline
//! - Screensaver modules provide fragment shaders or draw commands

use log::info;

/// Manages screensaver surfaces across monitors
pub struct Renderer {
    // TODO: Wayland connection, wgpu instance, active surfaces
}

impl Renderer {
    /// Create a new renderer connected to the Wayland display
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("Initializing Wayland renderer");
        // TODO:
        // 1. Connect to Wayland display
        // 2. Bind wlr-layer-shell-unstable-v1
        // 3. Initialize wgpu adapter/device
        Ok(Self {})
    }

    /// Start a screensaver on a specific monitor
    pub fn start_screensaver(
        &mut self,
        _output_name: &str,
        _screensaver: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO:
        // 1. Find the wl_output matching output_name
        // 2. Create a layer surface on that output (overlay layer, exclusive zone -1)
        // 3. Create wgpu surface + render pipeline for the screensaver
        // 4. Start the animation loop
        info!("Starting screensaver on {}", _output_name);
        Ok(())
    }

    /// Stop the screensaver on a specific monitor
    pub fn stop_screensaver(
        &mut self,
        _output_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO:
        // 1. Destroy the layer surface for this output
        // 2. Clean up wgpu resources
        info!("Stopping screensaver on {}", _output_name);
        Ok(())
    }

    /// Stop all active screensavers
    pub fn stop_all(&mut self) {
        info!("Stopping all screensavers");
        // TODO: Iterate and destroy all active surfaces
    }
}
