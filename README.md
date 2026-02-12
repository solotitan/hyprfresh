# HyprFresh

A native Wayland screensaver daemon for [Hyprland](https://hyprland.org) with **per-monitor idle detection**.

> **Status:** Early development (pre-alpha)

## Why

Wayland has no screensaver protocol. Hyprland has `hyprlock` (lock screen) and `hypridle` (session-wide idle), but neither provides actual screensavers, and neither tracks idle per-monitor. If you have two displays and you're working on one, the other just sits there.

HyprFresh fills that gap:
- Per-monitor idle tracking via Hyprland IPC (cursor position polling)
- Native `wlr-layer-shell` overlay surfaces for rendering
- GPU-accelerated screensaver animations via wgpu
- Built-in screensavers with a shader-based module system

## Features

- **Per-monitor idle detection** -- screensaver activates only on monitors you're not using
- **Session-wide idle** -- optional fallback using `ext-idle-notify-v1` for all monitors
- **Per-monitor configuration** -- different screensavers, timeouts, or disable per output
- **Built-in screensavers** -- see [Screensavers](#screensavers) below
- **WGSL shader system** -- screensavers are fragment shaders, easy to add new ones
- **Lightweight** -- single binary, no runtime dependencies beyond Wayland

## Requirements

- Hyprland (any recent version with IPC socket support)
- Wayland compositor with `wlr-layer-shell-unstable-v1` support
- GPU with Vulkan or OpenGL support (for wgpu)
- Rust toolchain (to build)

### Build Dependencies

```
# Arch Linux
sudo pacman -S rust wayland wayland-protocols

# Fedora
sudo dnf install rust cargo wayland-devel wayland-protocols-devel

# Ubuntu/Debian
sudo apt install rustc cargo libwayland-dev wayland-protocols
```

## Building

```bash
git clone https://github.com/solotitan/hyprfresh.git
cd hyprfresh
cargo build --release
```

## Installation

```bash
# Build + install binary, config, and systemd service
./install.sh
```

This installs:
- Binary: `~/.local/bin/hyprfresh`
- Config: `~/.config/hypr/hyprfresh.toml`
- Service: `~/.config/systemd/user/hyprfresh.service`

## Autostart

**Option A: systemd (recommended)**
```bash
systemctl --user enable --now hyprfresh.service
```

**Option B: hyprland.conf**
```conf
exec-once = hyprfresh
```

## Configuration

Edit `~/.config/hypr/hyprfresh.toml`:

```toml
[general]
idle_timeout = 300          # 5 minutes per-monitor
poll_interval = 1000        # Check cursor every 1s
session_idle = true         # Also use session-wide idle
session_idle_timeout = 600  # 10 min for all monitors

[screensaver]
name = "matrix"
fps = 30
opacity = 1.0

[screensaver.options]
speed = 1.0
color = [0.0, 1.0, 0.0]

# Per-monitor overrides
[monitors.DP-1]
idle_timeout = 120
screensaver = "starfield"

[monitors.HDMI-A-1]
disabled = true
```

## Usage

```bash
# Run the daemon
hyprfresh

# Run with verbose logging
hyprfresh --verbose

# Preview a screensaver (bypass idle detection)
hyprfresh --preview matrix

# List available screensavers
hyprfresh --list

# Use a custom config path
hyprfresh --config /path/to/config.toml
```

## Screensavers

### Matrix

Matrix digital rain -- green glyphs cascading down the screen.

<!-- Replace the src URL after uploading your recording to a GitHub issue/PR -->
<video src="https://github.com/user-attachments/assets/REPLACE_MATRIX" width="800" autoplay loop muted playsinline></video>

### Plasmula

Dark plasma waves -- electric purple (`#6000FF`), neon green (`#00FF6C`), deep teal, warm amber.

<video src="https://github.com/user-attachments/assets/REPLACE_PLASMULA" width="800" autoplay loop muted playsinline></video>

### Starfield

Classic starfield fly-through -- stars streaming past the camera.

<video src="https://github.com/user-attachments/assets/REPLACE_STARFIELD" width="800" autoplay loop muted playsinline></video>

### Blank

Black screen -- OLED-friendly, minimal power draw. No preview needed.

### Custom Shaders

Place `.wgsl` files in `~/.config/hypr/hyprfresh/shaders/`. The filename (without extension) becomes the screensaver name. Custom shaders override built-ins with the same name.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   HyprFresh                     │
│                                                 │
│  ┌──────────┐  ┌───────────┐  ┌──────────────┐  │
│  │   IPC    │  │   Idle    │  │   Renderer   │  │
│  │          │  │  Tracker  │  │              │  │
│  │ cursor   │──│ per-mon   │──│ layer-shell  │  │
│  │ monitors │  │ timers    │  │ wgpu/shaders │  │
│  └──────────┘  └───────────┘  └──────────────┘  │
│       │                             │           │
│  Hyprland IPC              Wayland Protocol     │
│  (UNIX socket)             (wlr-layer-shell)    │
└─────────────────────────────────────────────────┘
```

- **IPC module** -- Polls Hyprland's UNIX socket for cursor position and monitor info
- **Idle tracker** -- Maintains per-monitor idle timers, triggers screensaver start/stop
- **Renderer** -- Creates `wlr-layer-shell` overlay surfaces and renders screensaver shaders via wgpu
- **Screensaver modules** -- WGSL fragment shaders implementing the `Screensaver` trait

## Uninstall

```bash
./uninstall.sh
```

## Roadmap

- [x] Core idle detection daemon
- [x] wlr-layer-shell surface creation
- [x] wgpu rendering pipeline
- [x] Built-in screensavers (blank, matrix, plasmula, starfield)
- [x] Session-wide idle via ext-idle-notify-v1
- [x] Preview mode (`--preview`, `--monitor`, `--duration`)
- [x] Custom shader loading from `~/.config/hypr/hyprfresh/shaders/`
- [ ] Plugin system for external screensaver modules
- [ ] AUR package
- [ ] Nix flake

## License

GPL-3.0-or-later

## Contributing

This project is in early development. Issues and PRs welcome.
