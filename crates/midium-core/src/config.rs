use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::types::Mapping;

/// Top-level application configuration (config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub midi: MidiConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiConfig {
    /// How often to poll for new MIDI devices (seconds).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// How often to refresh audio device/session lists (seconds).
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_secs: u64,
    /// Volume change smoothing (0.0 = none, 1.0 = max).
    #[serde(default)]
    pub smoothing: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub plugin_dirs: Vec<PathBuf>,
}

/// Mappings file (mappings.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingsConfig {
    #[serde(default)]
    pub mappings: Vec<Mapping>,
}

// Defaults
fn default_true() -> bool { true }
fn default_log_level() -> String { "info".into() }
fn default_poll_interval() -> u64 { 2 }
fn default_refresh_interval() -> u64 { 5 }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            midi: MidiConfig::default(),
            audio: AudioConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            autostart: true,
            log_level: default_log_level(),
        }
    }
}

impl Default for MidiConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll_interval(),
            auto_connect: true,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            refresh_interval_secs: default_refresh_interval(),
            smoothing: 0.0,
        }
    }
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            plugin_dirs: Vec::new(),
        }
    }
}

/// Resolve the platform-specific config directory.
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs_path("Library/Application Support/midium")
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("midium")
        } else {
            dirs_path(".config/midium")
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            PathBuf::from(appdata).join("midium")
        } else {
            dirs_path("midium")
        }
    }
}

fn dirs_path(relative: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(relative)
}

/// Load AppConfig from the config directory, falling back to defaults.
pub fn load_config() -> anyhow::Result<AppConfig> {
    let path = config_dir().join("config.toml");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    } else {
        Ok(AppConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, AudioTarget, ControlId, ControlType, Mapping, ValueTransform};

    #[test]
    fn roundtrip_mapping_toml() {
        let mapping = Mapping {
            control: ControlId {
                device: "nanoKONTROL2 MIDI 1".into(),
                channel: 0,
                control_type: ControlType::CC(0),
            },
            action: Action::SetVolume {
                target: AudioTarget::SystemMaster,
            },
            transform: ValueTransform::Logarithmic,
        };
        let config = MappingsConfig { mappings: vec![mapping] };
        let toml_str = toml::to_string(&config).expect("serialize");
        println!("--- Serialized TOML ---\n{toml_str}---");
        let back: MappingsConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(back.mappings.len(), 1);
    }

    #[test]
    fn roundtrip_toggle_note() {
        let mapping = Mapping {
            control: ControlId {
                device: "nanoKONTROL2 MIDI 1".into(),
                channel: 0,
                control_type: ControlType::Note(48),
            },
            action: Action::ToggleMute {
                target: AudioTarget::SystemMaster,
            },
            transform: ValueTransform::Toggle,
        };
        let config = MappingsConfig { mappings: vec![mapping] };
        let toml_str = toml::to_string(&config).expect("serialize");
        println!("--- Toggle Note TOML ---\n{toml_str}---");
        let _back: MappingsConfig = toml::from_str(&toml_str).expect("deserialize");
    }
}

/// Load mappings from the config directory.
pub fn load_mappings() -> anyhow::Result<MappingsConfig> {
    let path = config_dir().join("mappings.toml");
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    } else {
        Ok(MappingsConfig {
            mappings: Vec::new(),
        })
    }
}
