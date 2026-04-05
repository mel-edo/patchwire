use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, debug};

/// Persisted to ~/.config/patchwire/state.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// Per sink-name: is linking enabled?
    /// This layer is separate from profiles so you cna make a one off toggle without modifying the saved profile
    pub sink_enabled: HashMap<String, bool>,
}

impl State {
    /// Load from ~/.config/patchwire/state.json
    /// Returns empty state if the file doesen't exist yet
    pub fn load() -> anyhow::Result<Self> {
        let path = state_path();

        if !path.exists() {
            return Ok(Self::default());
        }

        let text = std::fs::read_to_string(&path)?;
        let state = serde_json::from_str(&text)?;
        info!("loaded state from {}", path.display());
        Ok(state)
    }

    /// Persist to ~/.config/patchwire/state.json atomically
    /// Creates the config directory if it doesen't exist
    pub fn save(&self) -> anyhow::Result<()> {
        let path = state_path();

        // Ensure the directory exists before writing
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let text = serde_json::to_string_pretty(self)?;

        // write to a temp file then rename - avoids a corrupt state.json
        // if the process is killed mid write
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;

        debug!("saved state to {}", path.display());
        Ok(())
    }

    /// Set the enabled flag for a sink and immediately persist
    pub fn set_sink_enabled(&mut self, name: &str, enabled: bool) -> anyhow::Result<()> {
        self.sink_enabled.insert(name.to_string(), enabled);
        self.save()
    }

    pub fn is_sink_enabled(&self, name: &str) -> bool {
        self.sink_enabled.get(name).copied().unwrap_or(false)
    }
}

fn state_path() -> std::path::PathBuf {
    crate::config::Config::config_path()
        .parent()
        .expect("config path should have a parent directory")
        .join("state.json")
}