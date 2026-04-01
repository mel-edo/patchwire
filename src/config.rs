use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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