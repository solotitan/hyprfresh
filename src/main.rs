mod config;
mod idle;
mod ipc;
mod renderer;
mod screensavers;

use clap::Parser;
use log::{error, info};

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

#[tokio::main]
async fn main() {
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

    // Preview mode: run a screensaver immediately on all monitors
    if let Some(name) = cli.preview {
        info!("Preview mode: running screensaver '{}'", name);
        // TODO: Initialize renderer and run the named screensaver
        return;
    }

    // Main daemon loop
    info!("Starting idle monitor daemon");
    if let Err(e) = idle::run_daemon(cfg).await {
        error!("Daemon exited with error: {}", e);
        std::process::exit(1);
    }
}

/// Expand ~ to home directory in paths
fn shellexpand(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, stripped);
        }
    }
    path.to_string()
}
