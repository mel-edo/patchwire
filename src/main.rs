mod cli;
mod cli_client;
mod graph;
mod link_manager;
mod messages;
mod pw_thread;
mod config;
mod state;
mod dbus_server;

use std::sync::{Arc, Mutex};

use clap::Parser;
use tracing::{info, error};

fn main() -> anyhow::Result<()> {
    pipewire::init();
    // Initialise structured logging.
    // RUST_LOG=debug - verbose, info - normal

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "patchwire=info".into()),
        )
        .init();

    let cli = cli::Cli::parse();

    match cli.command {
        cli::Command::Daemon => {
            info!("patchwire daemon starting");
            run_daemon()?;
        }

        cli::Command::List => {
            run_async(cli_client::cmd_list())?;
        }

        cli::Command::Toggle { sink } => {
            run_async(cli_client::cmd_toggle(&sink))?;
        }

        cli::Command::Profile { name } => {
            run_async(cli_client::cmd_profile(&name))?;
        }
    }

    Ok(())
}

/// Run a single async future to completion on a throwaway tokio runtime
/// Used by CLI subcommands which are short lived - connect, call, print, exit
fn run_async<F>(fut: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    tokio::runtime::Runtime::new()?.block_on(fut)
}

fn run_daemon() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    let mut state = state::State::load()?;

    // Apply active profile into state on startup so state reflects what profile says for sinks we haven't seen a toggle for
    if let Some(profile_name) = &config.active_profile {
        if let Some(profile) = config.profiles.get(profile_name) {
            info!(%profile_name, "applying active profile on startup");
            for sink in &profile.enabled_sinks {
                state.sink_enabled.entry(sink.clone()).or_insert(true);
            }
        }
    }

    // wrap shared data in Arc<Mutex<>> so the D-Bus server and event loop can both access them
    let state = Arc::new(Mutex::new(state));
    let config = Arc::new(Mutex::new(config));
    let graph =  Arc::new(Mutex::new(graph::Graph::new()));

    // std::sync::mpsc for commands going down to PW thread
    // (PW thread is not async, so it uses blocking recv)
    let (cmd_tx, cmd_rx) = pipewire::channel::channel::<messages::PwCommand>();

    // tokio::sync::mpsc for events coming up from PW thread
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<messages::PwEvent>();

    let graph_for_pw = Arc::clone(&graph);
    // spawn the pw thread first - it blocks on its own main loop
    let pw_handle = std::thread::spawn(move || {
        if let Err(e) = pw_thread::run(cmd_rx, event_tx, graph_for_pw) {
            error!("Pipewire thread error: {e:#}");
        }
    });

    // tokio runtime for everything async: D-Bus, config I/O, CLI
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async move {
        info!("tokio runtime ready");

        if let Err(e) = dbus_server::run(
            Arc::clone(&state),
            Arc::clone(&config),
            Arc::clone(&graph),
            cmd_tx,
            event_rx,
        )
        .await
        {
            error!("D-Bus server error: {e:#}");
        }
    });

    pw_handle.join().expect("Pipewire thread panicked");
    Ok(())
}