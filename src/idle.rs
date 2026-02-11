//! Per-monitor idle detection
//!
//! Tracks cursor position over time to determine which monitors are idle.
//! Unlike session-wide idle (ext-idle-notify-v1), this provides per-monitor
//! granularity -- a monitor is considered idle if the cursor hasn't been
//! on it for the configured timeout period.

use crate::config::Config;
use crate::ipc;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::{self, Duration};

/// Per-monitor idle state
#[derive(Debug)]
struct MonitorIdleState {
    /// Last time the cursor was seen on this monitor
    last_active: Instant,
    /// Whether the screensaver is currently showing on this monitor
    screensaver_active: bool,
}

/// Run the main idle detection daemon loop
pub async fn run_daemon(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let poll_interval = Duration::from_millis(config.general.poll_interval);
    let idle_timeout = Duration::from_secs(config.general.idle_timeout);

    let mut monitor_states: HashMap<String, MonitorIdleState> = HashMap::new();
    let mut last_cursor_monitor: Option<String> = None;

    info!(
        "Idle daemon started: poll={}ms, timeout={}s",
        config.general.poll_interval, config.general.idle_timeout
    );

    let mut interval = time::interval(poll_interval);

    loop {
        interval.tick().await;

        // Get current cursor position and monitor layout
        let cursor = match ipc::get_cursor_pos().await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to get cursor position: {}", e);
                continue;
            }
        };

        let monitors = match ipc::get_monitors().await {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to get monitors: {}", e);
                continue;
            }
        };

        // Determine which monitor the cursor is on
        let current_monitor = ipc::cursor_on_monitor(&cursor, &monitors);

        // Initialize state for any new monitors
        let now = Instant::now();
        for monitor in &monitors {
            monitor_states
                .entry(monitor.name.clone())
                .or_insert(MonitorIdleState {
                    last_active: now,
                    screensaver_active: false,
                });
        }

        // Remove state for disconnected monitors
        let connected_names: Vec<String> = monitors.iter().map(|m| m.name.clone()).collect();
        monitor_states.retain(|name, state| {
            if !connected_names.contains(name) {
                if state.screensaver_active {
                    info!("Monitor {} disconnected, stopping screensaver", name);
                    // TODO: Stop screensaver on this monitor
                }
                false
            } else {
                true
            }
        });

        // Update the active monitor's last_active timestamp
        if let Some(ref name) = current_monitor {
            if let Some(state) = monitor_states.get_mut(name) {
                state.last_active = now;

                // If screensaver was active on this monitor, stop it
                if state.screensaver_active {
                    info!("Activity detected on {}, stopping screensaver", name);
                    state.screensaver_active = false;
                    // TODO: renderer::stop_screensaver(name)
                }
            }
        }

        // Check if cursor moved to a different monitor
        if current_monitor != last_cursor_monitor {
            if let Some(ref new_mon) = current_monitor {
                debug!("Cursor moved to monitor {}", new_mon);
            }
            last_cursor_monitor = current_monitor.clone();
        }

        // Check each monitor for idle timeout
        for (name, state) in monitor_states.iter_mut() {
            // Skip if monitor-specific config disables it
            if let Some(mon_cfg) = config.monitors.get(name) {
                if mon_cfg.disabled {
                    continue;
                }
            }

            // Use monitor-specific timeout or global default
            let timeout = config
                .monitors
                .get(name)
                .and_then(|m| m.idle_timeout)
                .map(Duration::from_secs)
                .unwrap_or(idle_timeout);

            let idle_duration = now.duration_since(state.last_active);

            if idle_duration >= timeout && !state.screensaver_active {
                info!(
                    "Monitor {} idle for {:.0}s (threshold: {:.0}s), starting screensaver",
                    name,
                    idle_duration.as_secs_f64(),
                    timeout.as_secs_f64()
                );
                state.screensaver_active = true;

                // Determine which screensaver to use
                let _screensaver_name = config
                    .monitors
                    .get(name)
                    .and_then(|m| m.screensaver.clone())
                    .unwrap_or_else(|| config.screensaver.name.clone());

                // TODO: renderer::start_screensaver(name, &screensaver_name, &config.screensaver)
            }
        }
    }
}
