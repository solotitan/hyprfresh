//! Per-monitor idle detection with ext-idle-notify-v1 as primary idle signal
//!
//! Architecture:
//! - **ext-idle-notify-v1** (compositor-level) is the authoritative idle signal.
//!   It correctly tracks ALL user input (keyboard, mouse, touch) — something we
//!   cannot replicate from userspace by polling cursor position alone.
//! - **Hyprland IPC** provides per-monitor targeting: we poll cursor position to
//!   know which monitors are inactive (cursor hasn't been there recently) and
//!   start screensavers on those first. The focused/active monitor only gets a
//!   screensaver when ext-idle-notify fires its Idled event.
//!
//! Flow:
//! 1. Poll loop tracks cursor position → knows which monitors are "inactive"
//! 2. Inactive monitors get screensavers after `idle_timeout` seconds
//! 3. When ext-idle-notify fires Idled → start screensaver on remaining monitors
//! 4. When ext-idle-notify fires Resumed → stop ALL screensavers
//! 5. Any cursor movement on a screensaver'd monitor → stop that screensaver

use crate::config::Config;
use crate::ipc::{self, HyprEvent};
use crate::renderer::RendererCommand;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

/// Per-monitor idle state
#[derive(Debug)]
struct MonitorIdleState {
    /// Last time the cursor was detected on this monitor
    last_cursor_seen: Instant,
    /// Whether the screensaver is currently showing on this monitor
    screensaver_active: bool,
}

/// Run the idle detection loop
///
/// Polls Hyprland IPC for cursor position and monitor layout.
/// Inactive monitors (cursor absent for `idle_timeout`) get screensavers.
/// The active monitor (where cursor/focus is) only gets a screensaver when
/// `session_idle_active` is set by ext-idle-notify-v1 in the renderer.
pub async fn run_idle_loop(
    config: Config,
    tx: mpsc::Sender<RendererCommand>,
    session_idle_active: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let poll_interval = Duration::from_millis(config.general.poll_interval);
    let idle_timeout = Duration::from_secs(config.general.idle_timeout);

    let mut monitor_states: HashMap<String, MonitorIdleState> = HashMap::new();
    let mut last_cursor_monitor: Option<String> = None;
    let mut last_cursor_pos: Option<(i32, i32)> = None;
    let mut session_was_idle = false;

    info!(
        "Idle loop started: poll={}ms, per-monitor timeout={}s",
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

        // Detect cursor movement
        let cursor_moved = match last_cursor_pos {
            Some((lx, ly)) => cursor.x != lx || cursor.y != ly,
            None => false,
        };
        last_cursor_pos = Some((cursor.x, cursor.y));

        // Which monitor is the cursor on?
        let current_monitor = ipc::cursor_on_monitor(&cursor, &monitors);

        // Initialize state for new monitors
        let now = Instant::now();
        for monitor in &monitors {
            monitor_states
                .entry(monitor.name.clone())
                .or_insert(MonitorIdleState {
                    last_cursor_seen: now,
                    screensaver_active: false,
                });
        }

        // Remove state for disconnected monitors
        let connected_names: Vec<String> = monitors.iter().map(|m| m.name.clone()).collect();
        monitor_states.retain(|name, state| {
            if !connected_names.contains(name) {
                if state.screensaver_active {
                    info!("Monitor {} disconnected, sending removal command", name);
                    let _ = tx.try_send(RendererCommand::MonitorRemoved {
                        monitor: name.clone(),
                    });
                }
                false
            } else {
                true
            }
        });

        // --- Cursor activity: update which monitor the cursor is on ---
        if let Some(ref name) = current_monitor {
            if let Some(state) = monitor_states.get_mut(name) {
                if cursor_moved {
                    state.last_cursor_seen = now;

                    // Wake this monitor if screensaver is active
                    if state.screensaver_active {
                        info!("Cursor movement on {}, stopping screensaver", name);
                        state.screensaver_active = false;
                        if let Err(e) = tx
                            .send(RendererCommand::Stop {
                                monitor: name.clone(),
                            })
                            .await
                        {
                            warn!("Failed to send stop command: {}", e);
                        }
                    }
                } else {
                    // Cursor is on this monitor but didn't move — still "present"
                    // (the user might be typing; ext-idle-notify handles that)
                    state.last_cursor_seen = now;
                }
            }
        }

        // Track monitor transitions
        if current_monitor != last_cursor_monitor {
            if let Some(ref new_mon) = current_monitor {
                debug!("Cursor moved to monitor {}", new_mon);
            }
            last_cursor_monitor = current_monitor.clone();
        }

        // --- Session-wide idle (ext-idle-notify-v1) ---
        let session_idle = session_idle_active.load(Ordering::SeqCst);

        // Session just went idle → start screensavers on ALL remaining monitors
        if session_idle && !session_was_idle {
            info!("Session idle detected, starting screensavers on all monitors");
            for (name, state) in monitor_states.iter_mut() {
                if state.screensaver_active {
                    continue;
                }
                if let Some(mon_cfg) = config.monitors.get(name) {
                    if mon_cfg.disabled {
                        continue;
                    }
                }

                let screensaver_name = config
                    .monitors
                    .get(name)
                    .and_then(|m| m.screensaver.clone())
                    .unwrap_or_else(|| config.screensaver.name.clone());

                info!("Session idle: starting screensaver on {}", name);
                state.screensaver_active = true;
                if let Err(e) = tx
                    .send(RendererCommand::Start {
                        monitor: name.clone(),
                        screensaver: screensaver_name,
                    })
                    .await
                {
                    warn!("Failed to send start command: {}", e);
                }
            }
        }

        // Session resumed → only stop the screensaver on the cursor's monitor.
        // Non-cursor monitors keep their screensavers — they're still idle
        // (cursor isn't there). The per-monitor idle block below handles them.
        if !session_idle && session_was_idle {
            if let Some(ref name) = current_monitor {
                info!("Session resumed, stopping screensaver on active monitor {}", name);
                if let Some(state) = monitor_states.get_mut(name) {
                    state.screensaver_active = false;
                    state.last_cursor_seen = now;
                }
                if let Err(e) = tx
                    .send(RendererCommand::Stop {
                        monitor: name.clone(),
                    })
                    .await
                {
                    warn!("Failed to send stop command: {}", e);
                }
            } else {
                info!("Session resumed but cursor not on any monitor, stopping all");
                for state in monitor_states.values_mut() {
                    state.screensaver_active = false;
                    state.last_cursor_seen = now;
                }
                if let Err(e) = tx.send(RendererCommand::StopAll).await {
                    warn!("Failed to send stop-all command: {}", e);
                }
            }
        }

        session_was_idle = session_idle;

        // --- Per-monitor idle: inactive monitors get screensavers early ---
        // Only when session is NOT idle (ext-idle-notify hasn't fired yet).
        // Once session goes idle, the block above covers everything.
        if !session_idle {
            for (name, state) in monitor_states.iter_mut() {
                if state.screensaver_active {
                    continue;
                }

                // Skip disabled monitors
                if let Some(mon_cfg) = config.monitors.get(name) {
                    if mon_cfg.disabled {
                        continue;
                    }
                }

                // Skip the monitor the cursor is currently on — that's the
                // "active" monitor. ext-idle-notify will handle it.
                if current_monitor.as_deref() == Some(name) {
                    continue;
                }

                // Per-monitor timeout
                let timeout = config
                    .monitors
                    .get(name)
                    .and_then(|m| m.idle_timeout)
                    .map(Duration::from_secs)
                    .unwrap_or(idle_timeout);

                let idle_duration = now.duration_since(state.last_cursor_seen);

                if idle_duration >= timeout {
                    let screensaver_name = config
                        .monitors
                        .get(name)
                        .and_then(|m| m.screensaver.clone())
                        .unwrap_or_else(|| config.screensaver.name.clone());

                    info!(
                        "Monitor {} inactive for {:.0}s (cursor elsewhere), starting screensaver",
                        name,
                        idle_duration.as_secs_f64()
                    );
                    state.screensaver_active = true;
                    if let Err(e) = tx
                        .send(RendererCommand::Start {
                            monitor: name.clone(),
                            screensaver: screensaver_name,
                        })
                        .await
                    {
                        warn!("Failed to send start command: {}", e);
                    }
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
                // The idle poll loop picks up new monitors via get_monitors()
            }
            HyprEvent::FocusedMonitor(name) => {
                debug!("Focus moved to monitor: {}", name);
                // Stop screensaver on the newly focused monitor (fast wake)
                let cmd = RendererCommand::Stop { monitor: name };
                if render_tx.send(cmd).await.is_err() {
                    break;
                }
            }
            HyprEvent::Workspace(_) | HyprEvent::Other(_) => {}
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
        drop(event_tx);

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
