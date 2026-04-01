mod cli;
mod config;
mod state;

use clap::Parser;
use tracing::info;

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
            // todo - wire this up to tokio + pw runtime
            println!("patchwork daemon - scaffold ok");
        }

        cli::Command::List => {
            println!("patchwork list - D-Bus client");
        }

        cli::Command::Toggle { sink } => {
            println!("patchwork toggle {sink} - D-Bus client");
        }

        cli::Command::Profile { name } => {
            println!("patchwork profile {name} - D-Bus client");
        }
    }

    Ok(())
}