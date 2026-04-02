use crate::graph::{NodeInfo, PortInfo};

/// Commands sent from the tokio side down to the PW thread
#[derive(Debug)]
pub enum PwCommand {
    /// Create monitor links from the current default sink to this target sink
    LinkSink { name: String },
    /// Destroy any active links to this target sink
    UnlinkSink { name: String },
    /// Shut down the Pipewire main loop cleanly
    Quit,
}

/// Events sent from the Pipewire thread up to the tokio side
#[derive(Debug)]
pub enum PwEvent {
    NodeAdded(NodeInfo),
    NodeRemoved(u32),
    PortAdded(PortInfo),
    PortRemoved(u32),
    /// A new audio sink appeared in the graph
    SinkAdded { name: String, description: String },
    /// An audio sink was removed from the graph
    SinkRemoved { name: String },
    /// The default sink changed
    DefaultChanged { name: String },
}