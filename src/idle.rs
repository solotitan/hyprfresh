//! Per-monitor idle detection
//!
//! Tracks cursor position over time to determine which monitors are idle.
//! Unlike session-wide idle (ext-idle-notify-v1), this provides per-monitor
//! granularity -- a monitor is considered idle if the cursor hasn't been
//! on it for the configured timeout period.
//!
//! Sends [`RendererCommand`]s to the renderer via an mpsc channel when
//! monitors transition between idle and active states.

use crate::config::Config;
use crate::ipc::{self, HyprEvent};
use crate::renderer::RendererCommand;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
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
///
/// Polls Hyprland IPC for cursor position and monitor layout, tracks
/// per-monitor idle time, and sends start/stop commands to the renderer.
pub async fn run_idle_loop(
    config: Config,
    tx: mpsc::Sender<RendererCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let poll_interval = Duration::from_millis(config.general.poll_interval);
    let idle_timeout = Duration::from_secs(config.general.idle_timeout);

    let mut monitor_states: HashMap<String, MonitorIdleState> = HashMap::new();
    let mut last_cursor_monitor: Option<String> = None;

    info!(
        "Idle loop started: poll={}ms, timeout={}s",
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
                    info!("Monitor {} disconnected, sending removal command", name);
                    let cmd = RendererCommand::MonitorRemoved {
                        monitor: name.clone(),
                    };
                    // Best-effort send; if renderer is gone we're shutting down anyway
                    let _ = tx.try_send(cmd);
                }
                false
            } else {
                true
            }
        });

        // Update the active monitor's last_active timestamp
        if let Some(ref name) = current_monitor
            && let Some(state) = monitor_states.get_mut(name)
        {
            state.last_active = now;

            // If screensaver was active on this monitor, stop it
            if state.screensaver_active {
                info!("Activity detected on {}, stopping screensaver", name);
                state.screensaver_active = false;
                let cmd = RendererCommand::Stop {
                    monitor: name.clone(),
                };
                if let Err(e) = tx.send(cmd).await {
                    warn!("Failed to send stop command: {}", e);
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
            if let Some(mon_cfg) = config.monitors.get(name)
                && mon_cfg.disabled
            {
                continue;
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
                let screensaver_name = config
                    .monitors
                    .get(name)
                    .and_then(|m| m.screensaver.clone())
                    .unwrap_or_else(|| config.screensaver.name.clone());

                let cmd = RendererCommand::Start {
                    monitor: name.clone(),
                    screensaver: screensaver_name,
                };
                if let Err(e) = tx.send(cmd).await {
                    warn!("Failed to send start command: {}", e);
                }
            }
        }
    }
}

/// Bridge Hyprland events into renderer commands.
///
/// Listens for events from the Hyprland event socket and translates
/// relevant ones (monitor hotplug, focus changes) into renderer commands.
/// Runs as a separate task alongside the idle poll loop.
pub async fn run_event_bridge(
    mut event_rx: mpsc::Receiver<HyprEvent>,
    render_tx: mpsc::Sender<RendererCommand>,
) {
    info!("Event bridge started");

    while let Some(event) = event_rx.recv().await {
        match event {
            HyprEvent::MonitorRemoved(name) => {
                info!("Monitor removed event: {}", name);
                let cmd = RendererCommand::MonitorRemoved { monitor: name };
                if render_tx.send(cmd).await.is_err() {
                    break;
                }
            }
            HyprEvent::MonitorAdded(name) => {
                info!("Monitor added event: {}", name);
                // No action needed -- the idle poll loop will pick up the
                // new monitor on its next iteration via get_monitors()
            }
            HyprEvent::FocusedMonitor(name) => {
                debug!("Focus moved to monitor: {}", name);
                // Activity on this monitor -- stop its screensaver if running.
                // The idle poll loop handles the authoritative state, but this
                // gives us faster wake response for focus changes.
                let cmd = RendererCommand::Stop { monitor: name };
                if render_tx.send(cmd).await.is_err() {
                    break;
                }
            }
            HyprEvent::Workspace(_) | HyprEvent::Other(_) => {
                // Workspace changes are informational; the idle loop
                // tracks cursor position which is the real activity signal.
            }
        }
    }

    info!("Event bridge stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the event bridge translates MonitorRemoved into a RendererCommand
    #[tokio::test]
    async fn event_bridge_monitor_removed() {
        let (event_tx, event_rx) = mpsc::channel(8);
        let (render_tx, mut render_rx) = mpsc::channel(8);

        let bridge = tokio::spawn(async move {
            run_event_bridge(event_rx, render_tx).await;
        });

        event_tx
            .send(HyprEvent::MonitorRemoved("DP-1".to_string()))
            .await
            .unwrap();
        drop(event_tx); // close channel so bridge exits

        let cmd = render_rx.recv().await.unwrap();
        match cmd {
            RendererCommand::MonitorRemoved { monitor } => assert_eq!(monitor, "DP-1"),
            other => panic!("Expected MonitorRemoved, got {:?}", other),
        }

        bridge.await.unwrap();
    }

    /// Verify the event bridge translates FocusedMonitor into a Stop command
    #[tokio::test]
    async fn event_bridge_focus_sends_stop() {
        let (event_tx, event_rx) = mpsc::channel(8);
        let (render_tx, mut render_rx) = mpsc::channel(8);

        let bridge = tokio::spawn(async move {
            run_event_bridge(event_rx, render_tx).await;
        });

        event_tx
            .send(HyprEvent::FocusedMonitor("HDMI-A-1".to_string()))
            .await
            .unwrap();
        drop(event_tx);

        let cmd = render_rx.recv().await.unwrap();
        match cmd {
            RendererCommand::Stop { monitor } => assert_eq!(monitor, "HDMI-A-1"),
            other => panic!("Expected Stop, got {:?}", other),
        }

        bridge.await.unwrap();
    }

    /// Verify workspace and unknown events don't produce renderer commands
    #[tokio::test]
    async fn event_bridge_ignores_workspace() {
        let (event_tx, event_rx) = mpsc::channel(8);
        let (render_tx, mut render_rx) = mpsc::channel(8);

        let bridge = tokio::spawn(async move {
            run_event_bridge(event_rx, render_tx).await;
        });

        event_tx
            .send(HyprEvent::Workspace("3".to_string()))
            .await
            .unwrap();
        event_tx
            .send(HyprEvent::Other("openwindow>>data".to_string()))
            .await
            .unwrap();
        event_tx
            .send(HyprEvent::MonitorAdded("DP-2".to_string()))
            .await
            .unwrap();
        drop(event_tx);

        bridge.await.unwrap();

        // None of those events should produce a renderer command
        assert!(render_rx.try_recv().is_err());
    }
}
