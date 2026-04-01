use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Persisted to ~/.config/patchwork/state.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    /// Per sink-name: is linking enabled?
    /// This layer is separate from profiles so you cna make a one off
    /// toggle without modifying the saved profile
    pub sink_enabled: HashMap<String, bool>,
}