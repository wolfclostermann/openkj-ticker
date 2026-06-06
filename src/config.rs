use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// Path to OpenKJ data directory. Auto-discovered if not set.
    pub data_dir: Option<PathBuf>,

    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub ticker: TickerConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default = "default_bind")]
    pub bind_address: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            bind_address: default_bind(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TickerConfig {
    /// How many singers to show in the ticker (after current + next up).
    #[serde(default = "default_singer_count")]
    pub singer_count: usize,

    /// How often to poll the database (milliseconds).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
}

impl Default for TickerConfig {
    fn default() -> Self {
        Self {
            singer_count: default_singer_count(),
            poll_interval_ms: default_poll_interval(),
        }
    }
}

fn default_port() -> u16 {
    8080
}
fn default_bind() -> String {
    "0.0.0.0".to_string()
}
fn default_singer_count() -> usize {
    8
}
fn default_poll_interval() -> u64 {
    1500
}

impl Config {
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read config: {}", path.display()))?;
            toml::from_str(&content)
                .with_context(|| format!("Failed to parse config: {}", path.display()))
        } else {
            tracing::info!(
                "No config file at {}, creating with defaults",
                path.display()
            );
            let cfg = Config {
                data_dir: None,
                server: ServerConfig::default(),
                ticker: TickerConfig::default(),
            };
            cfg.save(path)?;
            Ok(cfg)
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self).context("Failed to serialize config")?;
        let content = format!(
            "# openkj-ticker configuration\n\
             # Set data_dir to override auto-discovery.\n\
             # Example: data_dir = \"/home/user/.local/share/OpenKJ2\"\n\n{}",
            body
        );
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config: {}", path.display()))
    }
}

/// Search platform default locations for an OpenKJ data directory.
/// Returns the first candidate that contains openkj.sqlite or openkj2.ini.
pub fn discover_data_dir() -> Option<PathBuf> {
    let candidates = build_candidates()?;

    // Prefer a directory that already has the database or settings file.
    for candidate in &candidates {
        if candidate.join("openkj.sqlite").exists() || candidate.join("openkj2.ini").exists() {
            return Some(candidate.clone());
        }
    }

    // Fall back to any candidate directory that exists (OpenKJ installed but not yet run).
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn build_candidates() -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        let support = dirs::data_dir()?; // ~/Library/Application Support
        Some(vec![
            support.join("OpenKJ2"),
            support.join("OpenKJ2").join("OpenKJ2"),
            support.join("OpenKJ2-unstable"),
            support.join("OpenKJ"),
        ])
    }

    #[cfg(target_os = "linux")]
    {
        let data = dirs::data_dir()?; // ~/.local/share
        Some(vec![
            data.join("OpenKJ2"),
            data.join("OpenKJ2").join("OpenKJ2"),
            data.join("OpenKJ2-unstable"),
            data.join("OpenKJ"),
        ])
    }

    #[cfg(target_os = "windows")]
    {
        let roaming = dirs::data_dir()?; // %APPDATA%\Roaming
        Some(vec![
            roaming.join("OpenKJ2"),
            roaming.join("OpenKJ2").join("OpenKJ2"),
            roaming.join("OpenKJ2-unstable"),
            roaming.join("OpenKJ"),
        ])
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
