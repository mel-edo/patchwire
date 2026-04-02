use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// If true, automatically link any new sink that appears.
    /// If false (default), new sinks must be manually enabled.
    
    #[serde(default)]
    pub auto_link_new: bool,

    /// Named profiles. Key = profile name.
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,

    /// Name of the profile that was active at last save.
    pub active_profile: Option<String>,
}

/// A profile is a named set of sinks that should be linked
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    pub description: Option<String>,
    /// Sink node.name values that are enabled in this profile
    #[serde(default)]
    pub enabled_sinks: Vec<String>,
}

impl Config {
    /// Load from ~/.config/patchwork/config.toml
    /// Returns default config if the file doesen't exist yet
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();

        if !path.exists() {
            info!("no config file found at {}, using defaults", path.display());
            return Ok(Self::default());
        }

        let text = std::fs::read_to_string(&path)?;
        let config = toml::from_str(&text)?;
        info!("loaded config from {}", path.display());
        Ok(config)
    }

    pub fn config_dir() -> std::path::PathBuf {
        dirs::config_dir()
            .expect("could not find config directory")
            .join("patchwork")
    }
}

fn config_path() -> std::path::PathBuf {
    Config::config_dir().join("config.toml")
}