use clap::{Parser, Subcommand};

/// Patchwire - Pipewire audio output router
#[derive(Parser, Debug)]
#[command(name = "patchwire", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the routing daemon (run via systemd or manually)
    Daemon,

    /// List all detected audio sinks and their link state
    List,

    /// Toggle linking for a specific sink
    Toggle {
        /// Sink name (as shown by 'patchwire list')
        sink: String,
    },

    /// Switch to a saved profile
    Profile {
        /// Profile name
        name: String,
    },

    /// Volume control
    Volume {
        sink: String,
        /// Volume level from 0.0 to 1.0
        volume: f32,
    },
}