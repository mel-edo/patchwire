mod cli;
mod graph;
mod link_manager;
mod pw_thread;
mod config;
mod state;

use clap::Parser;
use tracing::{info, error};

fn main() -> anyhow::Result<()> {
    // Initialise structured logging.
    // RUST_LOG=debug - verbose, info - normal

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "patchwork=info".into()),
        )
        .init();

    let cli = cli::Cli::parse();

    match cli.command {
        cli::Command::Daemon => {
            info!("patchwork daemon starting");

            // Spawn a dedicated OS thread for PW main loop
            // PW objects are !Send so they must live and die on this thread
            let handle = std::thread::spawn(|| {
                if let Err(e) = pw_thread::run() {
                    error!("Pipewire thread error: {e:#}")
                }
            });

            handle.join().expect("Pipewire thread panicked");
        }

        cli::Command::List => {
            todo!("patchwork list - D-Bus client");
        }

        cli::Command::Toggle { sink } => {
            todo!("patchwork toggle {sink} - D-Bus client");
        }

        cli::Command::Profile { name } => {
            todo!("patchwork profile {name} - D-Bus client");
        }
    }

    Ok(())
}