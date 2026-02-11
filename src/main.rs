mod config;
mod idle;
mod ipc;
mod renderer;
mod screensavers;

use clap::Parser;
use log::{error, info, warn};
use renderer::{RendererCommand, WaylandState};
use smithay_client_toolkit::reexports::calloop::{self, EventLoop};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use tokio::sync::mpsc;

/// HyprFresh - A native Wayland screensaver daemon for Hyprland
#[derive(Parser, Debug)]
#[command(name = "hyprfresh", version, about)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "~/.config/hypr/hyprfresh.toml")]
    config: String,

    /// Run in verbose mode
    #[arg(short, long)]
    verbose: bool,

    /// Run a specific screensaver immediately (bypass idle detection)
    #[arg(short, long)]
    preview: Option<String>,

    /// List available screensavers
    #[arg(long)]
    list: bool,
}

fn main() {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    info!("HyprFresh v{} starting", env!("CARGO_PKG_VERSION"));

    // List screensavers and exit
    if cli.list {
        screensavers::list_available();
        return;
    }

    // Load config
    let config_path = shellexpand(&cli.config);
    let cfg = match config::Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config from {}: {}", config_path, e);
            info!("Using default configuration");
            config::Config::default()
        }
    };

    // Preview mode: run a screensaver immediately
    if let Some(ref name) = cli.preview {
        info!("Preview mode: running screensaver '{}'", name);
        run_preview(name);
        return;
    }

    // Daemon mode
    run_daemon(cfg);
}

/// Run the main daemon: Wayland renderer on main thread, tokio idle loop on background thread
fn run_daemon(cfg: config::Config) {
    // Initialize Wayland state and event queue
    let (mut state, event_queue, conn) = match WaylandState::new() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to initialize Wayland: {}", e);
            std::process::exit(1);
        }
    };

    // Create calloop event loop
    let mut event_loop: EventLoop<WaylandState> = match EventLoop::try_new() {
        Ok(el) => el,
        Err(e) => {
            error!("Failed to create event loop: {}", e);
            std::process::exit(1);
        }
    };

    let loop_handle = event_loop.handle();

    // Insert Wayland event source into calloop
    WaylandSource::new(conn, event_queue)
        .insert(loop_handle.clone())
        .expect("failed to insert Wayland source");

    // Create a calloop channel for receiving RendererCommands from tokio
    let (calloop_tx, calloop_rx) = calloop::channel::channel::<RendererCommand>();

    // Insert the channel receiver into the calloop event loop
    loop_handle
        .insert_source(calloop_rx, |event, _, state: &mut WaylandState| {
            if let calloop::channel::Event::Msg(cmd) = event {
                state.queue_command(cmd);
                state.process_commands();
            }
        })
        .expect("failed to insert command channel");

    // Spawn the tokio runtime on a background thread for the idle loop + IPC
    let idle_config = cfg.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime");

        rt.block_on(async move {
            // Channel: Hyprland events -> event bridge
            let (event_tx, event_rx) = mpsc::channel(64);

            let calloop_tx_idle = calloop_tx.clone();
            let calloop_tx_events = calloop_tx;

            // Spawn idle poll loop with a bridge to calloop
            let idle_handle = tokio::spawn(async move {
                let (tx, mut rx) = mpsc::channel::<RendererCommand>(32);

                // Forward tokio mpsc -> calloop channel
                let fwd = tokio::spawn(async move {
                    while let Some(cmd) = rx.recv().await {
                        if calloop_tx_idle.send(cmd).is_err() {
                            break;
                        }
                    }
                });

                if let Err(e) = idle::run_idle_loop(idle_config, tx).await {
                    error!("Idle loop exited with error: {}", e);
                }

                fwd.abort();
            });

            // Spawn Hyprland event listener
            let event_handle = tokio::spawn(async move {
                if let Err(e) = ipc::listen_events(event_tx).await {
                    warn!("Event listener exited: {}", e);
                }
            });

            // Spawn event bridge (HyprEvents -> RendererCommands -> calloop)
            let bridge_handle = tokio::spawn(async move {
                let (tx, mut rx) = mpsc::channel::<RendererCommand>(32);

                let fwd = tokio::spawn(async move {
                    while let Some(cmd) = rx.recv().await {
                        if calloop_tx_events.send(cmd).is_err() {
                            break;
                        }
                    }
                });

                idle::run_event_bridge(event_rx, tx).await;
                fwd.abort();
            });

            let _ = tokio::join!(idle_handle, event_handle, bridge_handle);
        });
    });

    // Run the calloop event loop on the main thread (Wayland requires this)
    info!("Starting Wayland event loop");
    loop {
        // Process any pending commands before dispatching
        // We need a QueueHandle but calloop manages that internally via WaylandSource.
        // Commands are processed via the calloop channel callback above.

        if let Err(e) = event_loop.dispatch(std::time::Duration::from_millis(16), &mut state) {
            error!("Event loop error: {}", e);
            break;
        }

        if state.exit {
            break;
        }
    }

    info!("HyprFresh shutting down");
}

/// Run a screensaver in preview mode (immediate, no idle detection)
fn run_preview(screensaver_name: &str) {
    let (mut state, event_queue, conn) = match WaylandState::new() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to initialize Wayland: {}", e);
            std::process::exit(1);
        }
    };

    let mut event_loop: EventLoop<WaylandState> = EventLoop::try_new()
        .expect("failed to create event loop");

    WaylandSource::new(conn, event_queue)
        .insert(event_loop.handle())
        .expect("failed to insert Wayland source");

    // Wait for outputs to be enumerated
    info!("Waiting for output enumeration...");
    for _ in 0..10 {
        let _ = event_loop.dispatch(std::time::Duration::from_millis(100), &mut state);
        if state.has_outputs() {
            break;
        }
    }

    if !state.has_outputs() {
        error!("No outputs found");
        std::process::exit(1);
    }

    // Start screensaver on all outputs
    let names = state.output_names();
    for name in &names {
        state.queue_command(RendererCommand::Start {
            monitor: name.clone(),
            screensaver: screensaver_name.to_string(),
        });
    }

    // Process the queued start commands
    state.process_commands();

    info!("Preview: {} on {} output(s). Press Ctrl+C to exit.", screensaver_name, names.len());

    loop {
        if let Err(e) = event_loop.dispatch(std::time::Duration::from_millis(16), &mut state) {
            error!("Event loop error: {}", e);
            break;
        }
        if state.exit {
            break;
        }
    }
}

/// Expand ~ to home directory in paths
fn shellexpand(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{}/{}", home, stripped);
    }
    path.to_string()
}
