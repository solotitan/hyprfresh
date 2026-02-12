use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

/// Top-level configuration for HyprFresh
#[derive(Debug, Default, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub monitors: HashMap<String, MonitorConfig>,

    #[serde(default)]
    pub screensaver: ScreensaverConfig,
}

/// General daemon settings
#[derive(Debug, Deserialize, Clone)]
pub struct GeneralConfig {
    /// Per-monitor idle timeout in seconds (default: 300 = 5 minutes)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,

    /// How often to poll cursor position in milliseconds (default: 1000)
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,

    /// Whether to also trigger on session-wide idle (via ext-idle-notify)
    #[serde(default = "default_true")]
    pub session_idle: bool,

    /// Session-wide idle timeout in seconds.
    /// Defaults to `idle_timeout` if not set, so the active monitor gets
    /// covered at the same time as inactive ones.
    pub session_idle_timeout: Option<u64>,
}

/// Per-monitor overrides
#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    /// Override idle timeout for this monitor
    pub idle_timeout: Option<u64>,

    /// Override screensaver for this monitor
    pub screensaver: Option<String>,

    /// Disable screensaver on this monitor
    #[serde(default)]
    pub disabled: bool,
}

/// Screensaver rendering settings
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct ScreensaverConfig {
    /// Which screensaver to use (default: "matrix")
    #[serde(default = "default_screensaver")]
    pub name: String,

    /// Target FPS for screensaver animation (default: 30)
    #[serde(default = "default_fps")]
    pub fps: u32,

    /// Opacity of the screensaver overlay (0.0 - 1.0, default: 1.0)
    #[serde(default = "default_opacity")]
    pub opacity: f32,

    /// Screensaver-specific options (passed to the screensaver module)
    #[serde(default)]
    pub options: HashMap<String, toml::Value>,
}

// Default value functions
fn default_idle_timeout() -> u64 {
    300
}
fn default_poll_interval() -> u64 {
    500
}
fn default_screensaver() -> String {
    "matrix".to_string()
}
fn default_fps() -> u32 {
    30
}
fn default_opacity() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}

impl GeneralConfig {
    /// Effective session idle timeout: explicit value or falls back to idle_timeout
    pub fn effective_session_idle_timeout(&self) -> u64 {
        self.session_idle_timeout.unwrap_or(self.idle_timeout)
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            idle_timeout: default_idle_timeout(),
            poll_interval: default_poll_interval(),
            session_idle: true,
            session_idle_timeout: None,
        }
    }
}

impl Default for ScreensaverConfig {
    fn default() -> Self {
        Self {
            name: default_screensaver(),
            fps: default_fps(),
            opacity: default_opacity(),
            options: HashMap::new(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_idle_defaults_to_idle_timeout() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.general.session_idle);
        assert_eq!(config.general.session_idle_timeout, None);
        // Effective timeout should match idle_timeout (300s default)
        assert_eq!(config.general.effective_session_idle_timeout(), 300);
    }

    #[test]
    fn session_idle_explicit_config() {
        let config: Config = toml::from_str(
            r#"
            [general]
            session_idle = false
            session_idle_timeout = 900
            "#,
        )
        .unwrap();
        assert!(!config.general.session_idle);
        assert_eq!(config.general.session_idle_timeout, Some(900));
        assert_eq!(config.general.effective_session_idle_timeout(), 900);
    }

    #[test]
    fn session_idle_inherits_idle_timeout() {
        let config: Config = toml::from_str(
            r#"
            [general]
            idle_timeout = 60
            session_idle = true
            "#,
        )
        .unwrap();
        assert!(config.general.session_idle);
        assert_eq!(config.general.session_idle_timeout, None);
        // Should inherit idle_timeout
        assert_eq!(config.general.effective_session_idle_timeout(), 60);
    }

    #[test]
    fn session_idle_explicit_overrides_idle_timeout() {
        let config: Config = toml::from_str(
            r#"
            [general]
            idle_timeout = 60
            session_idle = true
            session_idle_timeout = 120
            "#,
        )
        .unwrap();
        assert!(config.general.session_idle);
        assert_eq!(config.general.effective_session_idle_timeout(), 120);
    }
}
