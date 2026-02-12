//! Hyprland IPC interface
//!
//! Communicates with Hyprland via its UNIX socket to:
//! - Get cursor position
//! - Get monitor information (names, geometry, active workspace)
//! - Listen for events (workspace changes, monitor connects/disconnects)

use log::{debug, error, warn};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Information about a connected monitor
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub transform: u32,
    pub active_workspace_id: i32,
    pub focused: bool,
}

/// Cursor position
#[derive(Debug, Clone, Copy)]
pub struct CursorPos {
    pub x: i32,
    pub y: i32,
}

/// Get the Hyprland IPC socket path
fn socket_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")?;
    Ok(PathBuf::from(format!(
        "{}/hypr/{}/.socket.sock",
        xdg_runtime, his
    )))
}

/// Send a command to Hyprland and return the response
async fn hyprctl(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).await?;

    // Hyprland IPC protocol: send "j/<command>" for JSON output
    let msg = format!("j/{}", command);
    stream.write_all(msg.as_bytes()).await?;
    stream.shutdown().await?;

    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

/// Get current cursor position
pub async fn get_cursor_pos() -> Result<CursorPos, Box<dyn std::error::Error>> {
    let response = hyprctl("cursorpos").await?;
    let parsed: serde_json::Value =
        serde_json::from_str(&response).map_err(|e| format!("Failed to parse cursorpos: {}", e))?;

    Ok(CursorPos {
        x: parsed["x"].as_i64().unwrap_or(0) as i32,
        y: parsed["y"].as_i64().unwrap_or(0) as i32,
    })
}

/// Get information about all connected monitors
pub async fn get_monitors() -> Result<Vec<MonitorInfo>, Box<dyn std::error::Error>> {
    let response = hyprctl("monitors").await?;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&response).map_err(|e| format!("Failed to parse monitors: {}", e))?;

    let monitors = parsed
        .into_iter()
        .map(|m| {
            let raw_width = m["width"].as_u64().unwrap_or(0) as u32;
            let raw_height = m["height"].as_u64().unwrap_or(0) as u32;
            let transform = m["transform"].as_u64().unwrap_or(0) as u32;

            // Transforms 1, 3, 5, 7 are 90/270 degree rotations — swap w/h
            let (width, height) = if transform % 2 == 1 {
                (raw_height, raw_width)
            } else {
                (raw_width, raw_height)
            };

            MonitorInfo {
                id: m["id"].as_i64().unwrap_or(-1) as i32,
                name: m["name"].as_str().unwrap_or("unknown").to_string(),
                x: m["x"].as_i64().unwrap_or(0) as i32,
                y: m["y"].as_i64().unwrap_or(0) as i32,
                width,
                height,
                transform,
                active_workspace_id: m["activeWorkspace"]["id"].as_i64().unwrap_or(0) as i32,
                focused: m["focused"].as_bool().unwrap_or(false),
            }
        })
        .collect();

    Ok(monitors)
}

/// Determine which monitor the cursor is currently on
pub fn cursor_on_monitor(cursor: &CursorPos, monitors: &[MonitorInfo]) -> Option<String> {
    for monitor in monitors {
        if cursor.x >= monitor.x
            && cursor.x < monitor.x + monitor.width as i32
            && cursor.y >= monitor.y
            && cursor.y < monitor.y + monitor.height as i32
        {
            debug!("Cursor at ({}, {}) is on monitor {}", cursor.x, cursor.y, monitor.name);
            return Some(monitor.name.clone());
        }
    }
    warn!(
        "Cursor at ({}, {}) not found on any monitor",
        cursor.x, cursor.y
    );
    None
}

/// Get the Hyprland event socket path (socket2 for events)
fn event_socket_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let his = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")?;
    Ok(PathBuf::from(format!(
        "{}/hypr/{}/.socket2.sock",
        xdg_runtime, his
    )))
}

/// Hyprland events we care about
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum HyprEvent {
    /// A monitor was connected
    MonitorAdded(String),
    /// A monitor was disconnected
    MonitorRemoved(String),
    /// Active monitor changed (cursor moved between outputs)
    FocusedMonitor(String),
    /// Workspace changed on a monitor
    Workspace(String),
    /// Unknown/unhandled event
    Other(String),
}

/// Parse a raw Hyprland event line into a typed event
fn parse_event(line: &str) -> HyprEvent {
    // Hyprland event format: "EVENT>>DATA"
    if let Some((event, data)) = line.split_once(">>") {
        match event {
            "monitoradded" => HyprEvent::MonitorAdded(data.to_string()),
            // monitoraddedv2 format: "id,name,description"
            "monitoraddedv2" => {
                let name = data.split(',').nth(1).unwrap_or(data).to_string();
                HyprEvent::MonitorAdded(name)
            }
            "monitorremoved" => HyprEvent::MonitorRemoved(data.to_string()),
            // monitorremovedv2 format: "id,name"
            "monitorremovedv2" => {
                let name = data.split(',').nth(1).unwrap_or(data).to_string();
                HyprEvent::MonitorRemoved(name)
            }
            "focusedmon" => {
                // focusedmon>>MONNAME,WORKSPACE
                let monitor = data.split(',').next().unwrap_or(data).to_string();
                HyprEvent::FocusedMonitor(monitor)
            }
            "workspace" | "workspacev2" => HyprEvent::Workspace(data.to_string()),
            _ => HyprEvent::Other(line.to_string()),
        }
    } else {
        HyprEvent::Other(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_monitor_added() {
        match parse_event("monitoradded>>DP-1") {
            HyprEvent::MonitorAdded(name) => assert_eq!(name, "DP-1"),
            other => panic!("Expected MonitorAdded, got {:?}", other),
        }
    }

    #[test]
    fn parse_monitor_added_v2() {
        match parse_event("monitoraddedv2>>1,DP-2,some desc") {
            HyprEvent::MonitorAdded(name) => assert_eq!(name, "DP-2"),
            other => panic!("Expected MonitorAdded, got {:?}", other),
        }
    }

    #[test]
    fn parse_monitor_removed_v2() {
        match parse_event("monitorremovedv2>>1,HDMI-A-1") {
            HyprEvent::MonitorRemoved(name) => assert_eq!(name, "HDMI-A-1"),
            other => panic!("Expected MonitorRemoved, got {:?}", other),
        }
    }

    #[test]
    fn parse_monitor_removed() {
        match parse_event("monitorremoved>>HDMI-A-1") {
            HyprEvent::MonitorRemoved(name) => assert_eq!(name, "HDMI-A-1"),
            other => panic!("Expected MonitorRemoved, got {:?}", other),
        }
    }

    #[test]
    fn parse_focused_mon() {
        match parse_event("focusedmon>>DP-1,2") {
            HyprEvent::FocusedMonitor(name) => assert_eq!(name, "DP-1"),
            other => panic!("Expected FocusedMonitor, got {:?}", other),
        }
    }

    #[test]
    fn parse_workspace() {
        match parse_event("workspace>>3") {
            HyprEvent::Workspace(data) => assert_eq!(data, "3"),
            other => panic!("Expected Workspace, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_event() {
        match parse_event("openwindow>>some data") {
            HyprEvent::Other(raw) => assert_eq!(raw, "openwindow>>some data"),
            other => panic!("Expected Other, got {:?}", other),
        }
    }

    #[test]
    fn parse_malformed_line() {
        match parse_event("garbage with no separator") {
            HyprEvent::Other(raw) => assert_eq!(raw, "garbage with no separator"),
            other => panic!("Expected Other, got {:?}", other),
        }
    }

    #[test]
    fn cursor_on_monitor_hit() {
        let monitors = vec![
            MonitorInfo {
                id: 0,
                name: "DP-1".to_string(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                transform: 0,
                active_workspace_id: 1,
                focused: true,
            },
            MonitorInfo {
                id: 1,
                name: "DP-2".to_string(),
                x: 1920,
                y: 0,
                width: 2560,
                height: 1440,
                transform: 0,
                active_workspace_id: 2,
                focused: false,
            },
        ];

        let cursor = CursorPos { x: 100, y: 500 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-1".to_string()));

        let cursor = CursorPos { x: 2000, y: 700 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-2".to_string()));
    }

    #[test]
    fn cursor_on_monitor_miss() {
        let monitors = vec![MonitorInfo {
            id: 0,
            name: "DP-1".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            transform: 0,
            active_workspace_id: 1,
            focused: true,
        }];

        let cursor = CursorPos { x: 5000, y: 5000 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), None);
    }

    #[test]
    fn cursor_on_monitor_boundary() {
        let monitors = vec![MonitorInfo {
            id: 0,
            name: "DP-1".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            transform: 0,
            active_workspace_id: 1,
            focused: true,
        }];

        // Exactly at origin -- should be on monitor
        let cursor = CursorPos { x: 0, y: 0 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-1".to_string()));

        // At max edge -- should NOT be on monitor (exclusive upper bound)
        let cursor = CursorPos { x: 1920, y: 0 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), None);
    }

    #[test]
    fn cursor_on_rotated_monitor() {
        // Simulates: DP-3 portrait (transform=1, 2560x1440 -> 1440x2560) at x=-1440
        //            DP-2 landscape (transform=0, 2560x1440) at x=0
        // After transform swap, DP-3 spans x=-1440..0, DP-2 spans x=0..2560
        let monitors = vec![
            MonitorInfo {
                id: 1,
                name: "DP-3".to_string(),
                x: -1440,
                y: 0,
                width: 1440,  // already swapped by get_monitors()
                height: 2560,
                transform: 1,
                active_workspace_id: 6,
                focused: false,
            },
            MonitorInfo {
                id: 0,
                name: "DP-2".to_string(),
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
                transform: 0,
                active_workspace_id: 4,
                focused: true,
            },
        ];

        // Cursor on DP-2 (main monitor)
        let cursor = CursorPos { x: 720, y: 670 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-2".to_string()));

        // Cursor on DP-3 (portrait, left side)
        let cursor = CursorPos { x: -500, y: 670 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-3".to_string()));

        // Cursor at boundary between monitors (x=0 is DP-2, not DP-3)
        let cursor = CursorPos { x: 0, y: 500 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-2".to_string()));

        // Cursor at DP-3's right edge (x=-1 is still DP-3)
        let cursor = CursorPos { x: -1, y: 500 };
        assert_eq!(cursor_on_monitor(&cursor, &monitors), Some("DP-3".to_string()));
    }
}

/// Hide the hardware cursor by setting Hyprland's inactive_timeout to 1 second.
/// Called when screensavers activate so the cursor doesn't float above the overlay.
pub async fn hide_cursor() -> Result<(), Box<dyn std::error::Error>> {
    hyprctl_dispatch("keyword cursor:inactive_timeout 1").await?;
    Ok(())
}

/// Restore the hardware cursor by clearing Hyprland's inactive_timeout.
/// Called when all screensavers stop.
pub async fn show_cursor() -> Result<(), Box<dyn std::error::Error>> {
    hyprctl_dispatch("keyword cursor:inactive_timeout 0").await?;
    Ok(())
}

/// Send a raw command to Hyprland (non-JSON, for dispatchers/keywords)
async fn hyprctl_dispatch(command: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).await?;
    stream.write_all(command.as_bytes()).await?;
    stream.shutdown().await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

/// Listen for Hyprland events and send parsed events to the channel.
///
/// Connects to Hyprland's socket2 (event socket) and streams events
/// until the socket closes or the receiver is dropped.
pub async fn listen_events(
    tx: tokio::sync::mpsc::Sender<HyprEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = event_socket_path()?;
    let mut stream = UnixStream::connect(&path).await?;
    let mut buf = vec![0u8; 4096];

    debug!("Connected to Hyprland event socket at {:?}", path);

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            error!("Hyprland event socket closed");
            break;
        }
        let data = String::from_utf8_lossy(&buf[..n]);
        for line in data.lines() {
            if line.is_empty() {
                continue;
            }
            let event = parse_event(line);
            debug!("Hyprland event: {:?}", event);
            if tx.send(event).await.is_err() {
                // Receiver dropped, we're shutting down
                return Ok(());
            }
        }
    }

    Ok(())
}
