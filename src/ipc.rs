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
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
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
        .map(|m| MonitorInfo {
            id: m["id"].as_i64().unwrap_or(-1) as i32,
            name: m["name"].as_str().unwrap_or("unknown").to_string(),
            x: m["x"].as_i64().unwrap_or(0) as i32,
            y: m["y"].as_i64().unwrap_or(0) as i32,
            width: m["width"].as_u64().unwrap_or(0) as u32,
            height: m["height"].as_u64().unwrap_or(0) as u32,
            active_workspace_id: m["activeWorkspace"]["id"].as_i64().unwrap_or(0) as i32,
            focused: m["focused"].as_bool().unwrap_or(false),
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

/// Listen for Hyprland events (monitor connect/disconnect, workspace changes)
/// Calls the provided callback for each event line
pub async fn listen_events<F>(mut callback: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&str),
{
    let path = event_socket_path()?;
    let mut stream = UnixStream::connect(&path).await?;
    let mut buf = vec![0u8; 4096];

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            error!("Hyprland event socket closed");
            break;
        }
        let data = String::from_utf8_lossy(&buf[..n]);
        for line in data.lines() {
            callback(line);
        }
    }

    Ok(())
}
