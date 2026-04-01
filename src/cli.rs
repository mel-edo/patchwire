use clap::{Parser, Subcommand};

/// Patchwork - Pipewire audio output router
#[derive(Parser, Debug)]
#[command(name = "patchwork", version, about, long_about = None)]
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
        /// Sink name (as shown by 'patchwork list')
        sink: String,
    },

    /// Switch to a saved profile
    Profile {
        /// Profile name
        name: String,
    },
}