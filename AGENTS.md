# AGENTS.md — HyprFresh

Guidelines for agentic coding assistants working in this repository.

## Project Overview

**HyprFresh** is a native Wayland screensaver daemon for Hyprland with per-monitor idle detection. Written in Rust.

- **Repo:** `git@github.com:solotitan/hyprfresh.git`
- **Path:** `/home/solotitan/github/hyprfresh`
- **Status:** Pre-alpha (core architecture built, needs end-to-end testing)

## Communication Style

- **No task summaries** — Don't summarize what you did after completing tasks unless explicitly asked
- **Be concise** — Get to the point, skip the preamble
- **Show, don't tell** — Make the changes, then briefly confirm what was done (1-2 sentences max)
- **No emoji spam** — Use sparingly, only when it adds clarity

## Build & Test Commands

```bash
cargo check          # Type check (fast)
cargo build          # Debug build
cargo build --release # Release build (optimized, stripped)
cargo test           # Run all tests
cargo clippy         # Lint
```

No CI pipeline yet. Always run `cargo check && cargo test` before committing.

## Git Workflow

Uses **GitFlow-Lite** with `main` and `develop` branches.

### Branch Structure

| Branch | Purpose | Protected? |
|--------|---------|------------|
| `main` | Stable releases | Yes — user only |
| `develop` | Integration branch | Yes — user only |
| `feature/*` | Feature work | No |

### No Push Policy

**NEVER push to any remote branch. Ever.**

#### What You CAN Do
- `git commit` — Commit locally
- `cargo check && cargo test` — Build and test
- `git add` — Stage changes

#### What You CANNOT Do
- `git push` — Never push to origin/*
- `git push -u origin` — Never create remote branches
- `git push --force` — Never force push

#### How to Sync
When work is ready:
1. Show what was committed: `git status && git diff --stat`
2. Show recent commits: `git log --oneline -5`
3. Ask: "Ready to sync?"
4. Only push after user explicitly says "push" or "sync"

### Main Branch Policy

The `main` branch is completely off-limits. The user manages main manually.

## Architecture

```
hyprfresh/
├── Cargo.toml
├── config/hyprfresh.toml        # Default TOML config
├── install.sh / uninstall.sh
├── systemd/hyprfresh.service
├── screensavers/shaders/        # WGSL shader files
│   ├── common.wgsl              # Shared vertex shader + uniforms
│   ├── blank.wgsl               # Black screen
│   ├── matrix.wgsl              # Matrix digital rain
│   └── starfield.wgsl           # Starfield fly-through
└── src/
    ├── main.rs                  # CLI, calloop/tokio bridge, daemon/preview modes
    ├── config.rs                # TOML config parsing (general, per-monitor, screensaver)
    ├── ipc.rs                   # Hyprland UNIX socket IPC (cursorpos, monitors, events)
    ├── idle.rs                  # Per-monitor idle tracking + event bridge
    ├── renderer.rs              # SCTK Wayland state + wgpu rendering + layer shell
    └── screensavers/            # Rust screensaver trait + built-in modules
        ├── mod.rs               # Registry, trait definition, list/get
        ├── blank.rs
        ├── matrix.rs
        └── starfield.rs
```

### Threading Model

```
Main thread:  calloop event loop -> Wayland dispatch -> SCTK handlers -> wgpu render
Background:   tokio runtime -> idle poll loop + Hyprland event listener
Bridge:       tokio mpsc -> calloop::channel -> WaylandState.queue_command()
```

- **Wayland/wgpu** runs on the main thread (required by Wayland protocol)
- **Idle detection + Hyprland IPC** runs on tokio (async, background thread)
- **calloop::channel** bridges commands from tokio to the Wayland event loop

### Key Design Decisions

- **Rust** over C++ for better Wayland client libs and safety
- **Per-monitor idle** via polling `hyprctl cursorpos` + `hyprctl monitors` through Hyprland's UNIX socket (no Wayland protocol exists for per-monitor idle)
- **wlr-layer-shell** overlay surfaces for rendering (same approach as hyprlock)
- **wgpu** for GPU-accelerated rendering; screensavers are WGSL fragment shaders
- **smithay-client-toolkit (SCTK) v0.19** with delegate macro pattern for Wayland protocol handling
- **wayland-backend** with `client_system` feature for raw pointer access (needed for wgpu surface creation)
- **No `hyprland` crate** — raw IPC via tokio UnixStream (the crate has no stable release)

### Module Responsibilities

| Module | Role |
|--------|------|
| `ipc.rs` | Hyprland socket communication: `get_cursor_pos()`, `get_monitors()`, `cursor_on_monitor()`, `listen_events()`, `HyprEvent` parsing |
| `idle.rs` | `run_idle_loop()` polls IPC, tracks per-monitor idle state, sends `RendererCommand`s. `run_event_bridge()` translates `HyprEvent`s to commands |
| `renderer.rs` | `WaylandState` with SCTK delegates. Creates layer shell overlays, wgpu surfaces/pipelines. Processes commands, renders frames via frame callbacks |
| `config.rs` | TOML deserialization: `Config`, `GeneralConfig`, `MonitorConfig`, `ScreensaverConfig` |
| `screensavers/` | `Screensaver` trait + built-in modules. Real rendering uses WGSL shaders in `screensavers/shaders/` |

### Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `RendererCommand` | `renderer.rs` | Start/Stop/StopAll/MonitorRemoved/Shutdown — sent from idle to renderer |
| `HyprEvent` | `ipc.rs` | MonitorAdded/Removed/FocusedMonitor/Workspace/Other — from Hyprland socket2 |
| `WaylandState` | `renderer.rs` | Main SCTK state, implements CompositorHandler/OutputHandler/LayerShellHandler |
| `GpuContext` | `renderer.rs` | Shared wgpu instance/device/queue/buffers |
| `MonitorSurface` | `renderer.rs` | Per-monitor layer surface + wgpu surface + pipeline + uniforms |
| `Uniforms` | `renderer.rs` | GPU uniform buffer: `time: f32`, `resolution: vec2<f32>` |

## Dependencies (Key Crates)

| Crate | Version | Purpose |
|-------|---------|---------|
| `smithay-client-toolkit` | 0.19 | SCTK delegate pattern for Wayland protocols |
| `wayland-backend` | 0.3 (`client_system`) | Raw `wl_display*` / `wl_surface*` pointer access |
| `wgpu` | 24 | GPU rendering via Vulkan |
| `raw-window-handle` | 0.6 | Bridge between Wayland raw pointers and wgpu |
| `bytemuck` | 1 | Safe GPU buffer casting |
| `pollster` | 0.4 | Block on async wgpu init in sync context |
| `tokio` | 1 | Async runtime for idle loop + IPC |
| `calloop` | 0.14 | Wayland event loop (via SCTK) |
| `clap` | 4 | CLI argument parsing |
| `toml` / `serde` | 0.8 / 1 | Config file parsing |

## Current State & Known Issues

### What Works
- Full project scaffold compiles clean (14 expected dead-code warnings from stubs)
- 18 unit tests passing (IPC parsing, cursor geometry, event bridge, renderer commands, shader loading)
- Idle loop polls Hyprland IPC and sends RendererCommands via mpsc channel
- Event bridge translates Hyprland socket2 events to renderer commands
- Renderer creates SCTK layer shell surfaces with wgpu pipelines
- WGSL shaders for blank/matrix/starfield included
- calloop/tokio bridge wired up

### What Needs Testing
- End-to-end on a live Hyprland session (`hyprfresh --preview matrix`)
- Actual wgpu rendering on layer shell surfaces
- Monitor hotplug (connect/disconnect while running)
- Multi-monitor idle detection accuracy

### Remaining Roadmap
1. **End-to-end test** — idle detection triggers screensaver on correct monitor
2. **Session-wide idle** fallback via `ext-idle-notify-v1`
3. **Preview mode** polish (`--preview matrix`)
4. **Custom shader loading** from external files
5. **Clippy clean** pass
6. **AUR package / Nix flake**

### Warnings Breakdown (14 total)
- 6 from `screensavers/*.rs` — Rust trait/struct stubs not yet consumed by renderer
- 2 from `config.rs` — `session_idle`/`session_idle_timeout` fields (ext-idle-notify not implemented yet)
- 3 from `config.rs` — `fps`/`opacity`/`options` fields (not yet wired to renderer)
- 3 from `ipc.rs` — `MonitorInfo` fields + `HyprEvent` variant data (used in future features)

## For AI Agents

1. **Always run `cargo check && cargo test`** before considering work done
2. **Read the module you're changing** before editing — the architecture is intentional
3. **Wayland code runs on main thread** — never block it with async or heavy computation
4. **SCTK delegate pattern** — adding new Wayland protocol support requires: handler trait impl + delegate macro + registry_handlers entry
5. **Shaders are `include_str!`** — WGSL files in `screensavers/shaders/` are embedded at compile time
6. **Drop order matters** for Wayland — `wgpu_surface` before `layer_surface` before `connection`
7. **Don't create wgpu surfaces until first `configure` callback** — size is unknown before that
