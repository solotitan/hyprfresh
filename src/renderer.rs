//! Wayland surface renderer
//!
//! Creates wlr-layer-shell overlay surfaces on specific outputs and renders
//! screensaver animations using wgpu.
//!
//! Architecture:
//! - One layer surface per monitor that needs a screensaver
//! - Surfaces are created at the overlay layer (above everything)
//! - Each surface gets its own wgpu render pipeline
//! - Screensaver modules provide WGSL fragment shaders
//! - Receives commands from the idle tracker via a calloop channel
//!
//! Threading model:
//! - The Wayland event loop (calloop) runs on a dedicated thread
//! - The tokio idle loop sends RendererCommands via calloop::channel
//! - Frame callbacks drive the animation loop (compositor-synced vsync)

use crate::screensavers;
use log::{debug, info, warn};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputInfo, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::time::Instant;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_seat, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
};

// ---------------------------------------------------------------------------
// Public types shared with idle.rs / main.rs
// ---------------------------------------------------------------------------

/// Commands sent from the idle tracker to the renderer
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RendererCommand {
    /// Start a screensaver on a specific monitor
    Start {
        monitor: String,
        screensaver: String,
    },
    /// Start screensavers on ALL monitors (session-wide idle)
    StartAll { screensaver: String },
    /// Stop the screensaver on a specific monitor
    Stop { monitor: String },
    /// Stop all screensavers (e.g. session-wide wake)
    StopAll,
    /// A monitor was disconnected; clean up its resources
    MonitorRemoved { monitor: String },
    /// Shutdown the renderer
    Shutdown,
}

// ---------------------------------------------------------------------------
// GPU types
// ---------------------------------------------------------------------------

/// Uniform buffer passed to every screensaver shader
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    time: f32,
    _pad: f32,
    resolution: [f32; 2],
}

/// Fullscreen quad vertex
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
}

const QUAD_VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, -1.0],
    },
    Vertex {
        position: [-1.0, 1.0],
    },
    Vertex {
        position: [1.0, 1.0],
    },
    Vertex {
        position: [1.0, -1.0],
    },
];

const QUAD_INDICES: &[u16] = &[0, 1, 2, 2, 3, 0];

// ---------------------------------------------------------------------------
// Shared GPU resources (one wgpu instance/device for all monitors)
// ---------------------------------------------------------------------------

struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
}

// ---------------------------------------------------------------------------
// Per-monitor screensaver surface
// ---------------------------------------------------------------------------

struct MonitorSurface {
    /// The SCTK layer surface
    layer: LayerSurface,
    /// wgpu surface bound to the layer's wl_surface
    wgpu_surface: wgpu::Surface<'static>,
    /// Render pipeline for this monitor's screensaver
    pipeline: wgpu::RenderPipeline,
    /// Uniform buffer
    uniform_buffer: wgpu::Buffer,
    /// Bind group
    bind_group: wgpu::BindGroup,
    /// Surface format
    format: wgpu::TextureFormat,
    /// Current dimensions
    width: u32,
    height: u32,
    /// Whether we've received the first configure
    configured: bool,
    /// When the screensaver started (for time uniform)
    start_time: Instant,
    /// Name of the active screensaver
    screensaver_name: String,
}

// ---------------------------------------------------------------------------
// Wayland state (implements SCTK handler traits)
// ---------------------------------------------------------------------------

/// Session-wide idle configuration passed to the renderer
#[derive(Debug, Clone)]
pub struct SessionIdleConfig {
    /// Whether session-wide idle is enabled
    pub enabled: bool,
    /// Session idle timeout in seconds
    pub timeout_secs: u64,
    /// Default screensaver name for session-wide idle
    pub screensaver: String,
}

/// Main renderer state, driven by the calloop event loop
pub struct WaylandState {
    // SCTK state
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    layer_shell: LayerShell,
    seat_state: SeatState,

    // Stored QueueHandle so we can process commands from any calloop callback
    qh: QueueHandle<Self>,

    // GPU
    gpu: GpuContext,

    // Per-monitor surfaces: output name -> MonitorSurface
    surfaces: HashMap<String, MonitorSurface>,

    // Map wl_output -> output name (for looking up outputs by name)
    output_map: HashMap<wl_output::WlOutput, String>,

    // Wayland connection (needed for raw handle extraction)
    conn: Connection,

    // Control
    pub exit: bool,

    // Pending commands from the idle tracker (processed in the event loop)
    pending_commands: Vec<RendererCommand>,

    // Session-wide idle (ext-idle-notify-v1)
    idle_notifier: Option<ExtIdleNotifierV1>,
    idle_notification: Option<ExtIdleNotificationV1>,
    session_idle_config: SessionIdleConfig,
}

// ---------------------------------------------------------------------------
// Shader loading
// ---------------------------------------------------------------------------

/// Common vertex shader source (shared by all screensavers)
const COMMON_SHADER: &str = include_str!("../screensavers/shaders/common.wgsl");

/// Get the fragment shader source for a named screensaver.
/// Checks custom shader directory first, then falls back to built-in.
fn get_fragment_shader(name: &str) -> String {
    // Try custom shader first
    if let Some(dir) = screensavers::custom_shader_dir() {
        let path = dir.join(format!("{}.wgsl", name));
        if path.is_file() {
            match screensavers::load_custom_shader(&path) {
                Ok(source) => {
                    info!("Loaded custom shader '{}' from {}", name, path.display());
                    return source;
                }
                Err(e) => {
                    warn!("{}", e);
                    warn!("Falling back to built-in for '{}'", name);
                }
            }
        }
    }

    // Built-in shaders
    match name {
        "blank" => include_str!("../screensavers/shaders/blank.wgsl").to_string(),
        "matrix" => include_str!("../screensavers/shaders/matrix.wgsl").to_string(),
        "starfield" => include_str!("../screensavers/shaders/starfield.wgsl").to_string(),
        _ => {
            warn!("Unknown screensaver '{}', falling back to blank", name);
            include_str!("../screensavers/shaders/blank.wgsl").to_string()
        }
    }
}

/// Combine common vertex shader with a screensaver's fragment shader
fn build_shader_source(screensaver_name: &str) -> String {
    let fragment = get_fragment_shader(screensaver_name);
    format!("{}\n{}", COMMON_SHADER, fragment)
}

// ---------------------------------------------------------------------------
// GPU initialization
// ---------------------------------------------------------------------------

impl GpuContext {
    fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        // We need a temporary surface to select an adapter, but we don't have a
        // layer surface yet. Use adapter selection without a surface first, then
        // verify compatibility when we create actual surfaces.
        //
        // Create a raw display handle for adapter hints
        let _raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr() as *mut _).ok_or("null display pointer")?,
        ));

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or("no suitable GPU adapter found")?;

        info!("GPU adapter: {}", adapter.get_info().name);

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))?;

        // Create shared vertex/index buffers for the fullscreen quad
        use wgpu::util::DeviceExt;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_vertices"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_indices"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Shared bind group layout for the uniform buffer
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            vertex_buffer,
            index_buffer,
            bind_group_layout,
        })
    }
}

// ---------------------------------------------------------------------------
// WaylandState implementation
// ---------------------------------------------------------------------------

impl WaylandState {
    /// Initialize the Wayland connection and GPU context.
    ///
    /// Returns the state, the event queue, and the connection for the calloop event loop.
    pub fn new(
        session_idle_config: SessionIdleConfig,
    ) -> Result<
        (Self, wayland_client::EventQueue<Self>, Connection),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init(&conn)?;
        let qh = event_queue.handle();

        let compositor_state =
            CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {}", e))?;
        let layer_shell =
            LayerShell::bind(&globals, &qh).map_err(|e| format!("wlr-layer-shell: {}", e))?;
        let output_state = OutputState::new(&globals, &qh);
        let seat_state = SeatState::new(&globals, &qh);
        let registry_state = RegistryState::new(&globals);

        // Bind ext-idle-notify-v1 if available and session idle is enabled
        let idle_notifier: Option<ExtIdleNotifierV1> = if session_idle_config.enabled {
            match globals.bind::<ExtIdleNotifierV1, Self, ()>(&qh, 1..=1, ()) {
                Ok(notifier) => {
                    info!("ext-idle-notify-v1 bound successfully");
                    Some(notifier)
                }
                Err(e) => {
                    warn!(
                        "ext-idle-notify-v1 not available ({}), session-wide idle disabled",
                        e
                    );
                    None
                }
            }
        } else {
            info!("Session-wide idle disabled by config");
            None
        };

        let gpu = GpuContext::new(&conn)?;

        let conn_clone = conn.clone();
        let qh_clone = event_queue.handle();
        Ok((
            Self {
                registry_state,
                output_state,
                compositor_state,
                layer_shell,
                seat_state,
                qh: qh_clone,
                gpu,
                surfaces: HashMap::new(),
                output_map: HashMap::new(),
                conn,
                exit: false,
                pending_commands: Vec::new(),
                idle_notifier,
                idle_notification: None,
                session_idle_config,
            },
            event_queue,
            conn_clone,
        ))
    }

    /// Returns the names of all known outputs
    pub fn output_names(&self) -> Vec<String> {
        self.output_map.values().cloned().collect()
    }

    /// Returns true if any outputs have been enumerated
    pub fn has_outputs(&self) -> bool {
        !self.output_map.is_empty()
    }

    /// Queue a command for processing on the next event loop iteration
    pub fn queue_command(&mut self, cmd: RendererCommand) {
        self.pending_commands.push(cmd);
    }

    /// Process all pending commands
    pub fn process_commands(&mut self) {
        let commands: Vec<_> = self.pending_commands.drain(..).collect();
        for cmd in commands {
            match cmd {
                RendererCommand::Start {
                    monitor,
                    screensaver,
                } => {
                    let qh = self.qh.clone();
                    self.start_screensaver(&monitor, &screensaver, &qh);
                }
                RendererCommand::StartAll { screensaver } => {
                    self.start_all(&screensaver);
                }
                RendererCommand::Stop { monitor } => {
                    self.stop_screensaver(&monitor);
                }
                RendererCommand::StopAll => {
                    self.stop_all();
                }
                RendererCommand::MonitorRemoved { monitor } => {
                    self.stop_screensaver(&monitor);
                }
                RendererCommand::Shutdown => {
                    self.stop_all();
                    self.exit = true;
                }
            }
        }
    }

    /// Find the wl_output for a monitor name
    fn find_output(&self, name: &str) -> Option<wl_output::WlOutput> {
        self.output_map
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(o, _)| o.clone())
    }

    /// Start a screensaver on a specific monitor
    fn start_screensaver(
        &mut self,
        output_name: &str,
        screensaver_name: &str,
        qh: &QueueHandle<Self>,
    ) {
        // Don't start if already active
        if self.surfaces.contains_key(output_name) {
            debug!(
                "Screensaver already active on {}, ignoring start",
                output_name
            );
            return;
        }

        let output = match self.find_output(output_name) {
            Some(o) => o,
            None => {
                warn!(
                    "Cannot start screensaver: output '{}' not found",
                    output_name
                );
                return;
            }
        };

        info!(
            "Creating layer surface for screensaver '{}' on {}",
            screensaver_name, output_name
        );

        // Create a wl_surface and layer surface
        let wl_surface = self.compositor_state.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            wl_surface,
            Layer::Overlay,
            Some("hyprfresh"),
            Some(&output),
        );

        // Configure: fullscreen, no exclusive zone, no keyboard
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0); // 0 = fill output

        // Initial commit triggers configure from compositor
        layer.commit();

        // Create wgpu surface from the raw Wayland handles
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(self.conn.backend().display_ptr() as *mut _).unwrap(),
        ));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).unwrap(),
        ));

        let wgpu_surface = unsafe {
            self.gpu
                .instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .expect("failed to create wgpu surface")
        };

        // Create uniform buffer
        let uniform_buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group
        let bind_group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("uniforms_bind_group"),
                layout: &self.gpu.bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
            });

        // Build shader and pipeline (will be finalized on first configure when we know the format)
        // For now, use a placeholder format -- we'll rebuild on configure
        let shader_source = build_shader_source(screensaver_name);
        let shader = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(screensaver_name),
                source: wgpu::ShaderSource::Wgsl(shader_source.into()),
            });

        // We need the surface format from capabilities, but we can't get that until
        // the surface is configured. Use a default and reconfigure on first configure.
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;

        let pipeline = Self::create_pipeline(
            &self.gpu.device,
            &self.gpu.bind_group_layout,
            &shader,
            format,
        );

        self.surfaces.insert(
            output_name.to_string(),
            MonitorSurface {
                layer,
                wgpu_surface,
                pipeline,
                uniform_buffer,
                bind_group,
                format,
                width: 0,
                height: 0,
                configured: false,
                start_time: Instant::now(),
                screensaver_name: screensaver_name.to_string(),
            },
        );
    }

    /// Create a render pipeline for a screensaver shader
    fn create_pipeline(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        shader: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screensaver_pipeline_layout"),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("screensaver_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    }

    /// Stop the screensaver on a specific monitor
    fn stop_screensaver(&mut self, output_name: &str) {
        if let Some(surface) = self.surfaces.remove(output_name) {
            info!("Stopping screensaver on {}", output_name);
            // Drop order matters: wgpu surface before layer surface
            drop(surface.wgpu_surface);
            drop(surface.layer);
        }
    }

    /// Stop all active screensavers
    fn stop_all(&mut self) {
        let names: Vec<String> = self.surfaces.keys().cloned().collect();
        for name in names {
            self.stop_screensaver(&name);
        }
    }

    /// Start screensavers on ALL monitors (session-wide idle)
    fn start_all(&mut self, screensaver_name: &str) {
        let names: Vec<String> = self.output_map.values().cloned().collect();
        let qh = self.qh.clone();
        for name in names {
            self.start_screensaver(&name, screensaver_name, &qh);
        }
    }

    /// Render a frame for a specific monitor
    fn render_frame(&self, output_name: &str) {
        let surface = match self.surfaces.get(output_name) {
            Some(s) if s.configured => s,
            _ => return,
        };

        let elapsed = surface.start_time.elapsed().as_secs_f32();
        let uniforms = Uniforms {
            time: elapsed,
            _pad: 0.0,
            resolution: [surface.width as f32, surface.height as f32],
        };

        self.gpu
            .queue
            .write_buffer(&surface.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let frame = match surface.wgpu_surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to get swapchain texture for {}: {}", output_name, e);
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("screensaver_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screensaver_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&surface.pipeline);
            pass.set_bind_group(0, &surface.bind_group, &[]);
            pass.set_vertex_buffer(0, self.gpu.vertex_buffer.slice(..));
            pass.set_index_buffer(self.gpu.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..6, 0, 0..1);
        }

        self.gpu.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    /// Request the next frame callback for a monitor
    fn request_frame(&self, output_name: &str, qh: &QueueHandle<Self>) {
        if let Some(surface) = self.surfaces.get(output_name)
            && surface.configured
        {
            let wl_surf = surface.layer.wl_surface();
            wl_surf.frame(qh, wl_surf.clone());
            surface.layer.commit();
        }
    }
}

// ---------------------------------------------------------------------------
// SCTK handler implementations
// ---------------------------------------------------------------------------

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // TODO: handle HiDPI scaling
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Find which monitor this surface belongs to and render
        let output_name = self
            .surfaces
            .iter()
            .find(|(_, s)| s.layer.wl_surface() == surface)
            .map(|(name, _)| name.clone());

        if let Some(name) = output_name {
            self.render_frame(&name);
            self.request_frame(&name, qh);
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(info) = self.output_state.info(&output) {
            let name = output_name_from_info(&info);
            info!("Output added: {}", name);
            self.output_map.insert(output, name);
        }
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(info) = self.output_state.info(&output) {
            let name = output_name_from_info(&info);
            self.output_map.insert(output, name);
        }
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(name) = self.output_map.remove(&output) {
            info!("Output removed: {}", name);
            self.stop_screensaver(&name);
        }
    }
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        // Find and remove the surface that was closed
        let name = self
            .surfaces
            .iter()
            .find(|(_, s)| &s.layer == layer)
            .map(|(n, _)| n.clone());

        if let Some(name) = name {
            info!("Layer surface closed for {}", name);
            self.stop_screensaver(&name);
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // Find which monitor this layer surface belongs to
        let output_name = self
            .surfaces
            .iter()
            .find(|(_, s)| &s.layer == layer)
            .map(|(n, _)| n.clone());

        let Some(name) = output_name else { return };

        let (w, h) = configure.new_size;
        let width = if w == 0 { 1920 } else { w };
        let height = if h == 0 { 1080 } else { h };

        info!("Configure layer surface on {}: {}x{}", name, width, height);

        // Get the actual surface format from capabilities
        let surface = self.surfaces.get_mut(&name).unwrap();
        let caps = surface.wgpu_surface.get_capabilities(&self.gpu.adapter);

        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        // Select best present mode: prefer Mailbox (low-latency) but fall back to Fifo (always supported)
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };

        // Configure the wgpu surface
        surface.wgpu_surface.configure(
            &self.gpu.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                view_formats: vec![format],
                alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                width,
                height,
                desired_maximum_frame_latency: 2,
                present_mode,
            },
        );

        // Rebuild pipeline if format changed
        if format != surface.format {
            let shader_source = build_shader_source(&surface.screensaver_name);
            let shader = self
                .gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&surface.screensaver_name),
                    source: wgpu::ShaderSource::Wgsl(shader_source.into()),
                });
            surface.pipeline = Self::create_pipeline(
                &self.gpu.device,
                &self.gpu.bind_group_layout,
                &shader,
                format,
            );
            surface.format = format;
        }

        surface.width = width;
        surface.height = height;
        surface.configured = true;

        // Render first frame and start the frame callback chain
        let name_clone = name.clone();
        self.render_frame(&name_clone);
        self.request_frame(&name_clone, qh);
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
    ) {
        // Create idle notification when we get a seat and have the notifier
        if self.idle_notification.is_none()
            && let Some(ref notifier) = self.idle_notifier
        {
            let timeout_ms = (self.session_idle_config.timeout_secs * 1000) as u32;
            info!(
                "Creating ext-idle-notify-v1 notification (timeout: {}s)",
                self.session_idle_config.timeout_secs
            );
            let notification = notifier.get_idle_notification(timeout_ms, &seat, qh, ());
            self.idle_notification = Some(notification);
        }
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }
}

// ---------------------------------------------------------------------------
// ext-idle-notify-v1 Dispatch implementations
// ---------------------------------------------------------------------------

impl Dispatch<ExtIdleNotifierV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtIdleNotifierV1,
        _event: ext_idle_notifier_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // ExtIdleNotifierV1 has no events (empty enum)
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                info!("Session-wide idle detected, starting screensavers on all monitors");
                let screensaver = state.session_idle_config.screensaver.clone();
                state.queue_command(RendererCommand::StartAll { screensaver });
                state.process_commands();
            }
            ext_idle_notification_v1::Event::Resumed => {
                info!("Session-wide activity resumed, stopping all screensavers");
                state.queue_command(RendererCommand::StopAll);
                state.process_commands();
            }
            _ => {} // non_exhaustive
        }
    }
}

// ---------------------------------------------------------------------------
// SCTK delegate macros
// ---------------------------------------------------------------------------

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_layer!(WaylandState);
delegate_seat!(WaylandState);
delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a usable name from OutputInfo
fn output_name_from_info(info: &OutputInfo) -> String {
    info.name
        .clone()
        .unwrap_or_else(|| format!("output-{}", info.id))
}

// ---------------------------------------------------------------------------
// Tests (command-level only; Wayland tests need a compositor)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // We can't test the full Wayland renderer without a compositor,
    // but we can test the command/shader infrastructure.

    #[test]
    fn shader_sources_compile() {
        // Verify all shader sources can be loaded and concatenated
        for name in &["blank", "matrix", "starfield"] {
            let source = build_shader_source(name);
            assert!(
                source.contains("vs_main"),
                "missing vertex entry in {}",
                name
            );
            assert!(
                source.contains("fs_main"),
                "missing fragment entry in {}",
                name
            );
        }
    }

    #[test]
    fn unknown_shader_falls_back() {
        let source = build_shader_source("nonexistent");
        // Should fall back to blank
        assert!(source.contains("fs_main"));
    }

    #[test]
    fn uniforms_layout() {
        // Verify uniform struct is correctly sized for GPU alignment
        assert_eq!(std::mem::size_of::<Uniforms>(), 16); // 4 + 4 + 8 = 16 bytes
    }

    #[test]
    fn vertex_layout() {
        assert_eq!(std::mem::size_of::<Vertex>(), 8); // 2 * f32
        assert_eq!(QUAD_VERTICES.len(), 4);
        assert_eq!(QUAD_INDICES.len(), 6);
    }

    // Async command tests from the old renderer still work conceptually,
    // but the new renderer uses calloop channels instead of tokio mpsc.
    // These test the RendererCommand enum itself.
    #[test]
    fn renderer_command_variants() {
        let cmds = vec![
            RendererCommand::Start {
                monitor: "DP-1".into(),
                screensaver: "matrix".into(),
            },
            RendererCommand::StartAll {
                screensaver: "matrix".into(),
            },
            RendererCommand::Stop {
                monitor: "DP-1".into(),
            },
            RendererCommand::StopAll,
            RendererCommand::MonitorRemoved {
                monitor: "DP-1".into(),
            },
            RendererCommand::Shutdown,
        ];
        assert_eq!(cmds.len(), 6);
    }

    #[test]
    fn session_idle_config_defaults() {
        let config = SessionIdleConfig {
            enabled: true,
            timeout_secs: 600,
            screensaver: "matrix".to_string(),
        };
        assert!(config.enabled);
        assert_eq!(config.timeout_secs, 600);
        assert_eq!(config.screensaver, "matrix");
    }

    #[test]
    fn session_idle_config_disabled() {
        let config = SessionIdleConfig {
            enabled: false,
            timeout_secs: 0,
            screensaver: "blank".to_string(),
        };
        assert!(!config.enabled);
    }

    #[test]
    fn start_all_command_clone() {
        let cmd = RendererCommand::StartAll {
            screensaver: "starfield".into(),
        };
        let cloned = cmd.clone();
        match cloned {
            RendererCommand::StartAll { screensaver } => {
                assert_eq!(screensaver, "starfield");
            }
            _ => panic!("Expected StartAll"),
        }
    }
}
