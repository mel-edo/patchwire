mod cli;
mod graph;
mod link_manager;
mod messages;
mod pw_thread;
mod config;
mod state;

use clap::Parser;
use tracing::{info, error, warn};

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
            run_daemon()?;
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

    // std::sync::mpsc for commands going down to PW thread
    // (PW thread is not async, so it uses blocking recv)
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<messages::PwCommand>();

    // tokio::sync::mpsc for events coming up from PW thread
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<messages::PwEvent>();

    // spawn the pw thread first - it blocks on its own main loop
    let pw_handle = std::thread::spawn(move || {
        if let Err(e) = pw_thread::run(cmd_rx, event_tx) {
            error!("Pipewire thread error: {e:#}");
        }
    });

    // tokio runtime for everything async: D-Bus, config I/O, CLI
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async move {
        info!("tokio runtime ready");
        let mut current_default: Option<String> = None;

        // event loop: consumes PwEvents from PW thread
        while let Some(event) = event_rx.recv().await {
            match event {
                messages::PwEvent::SinkAdded { name, description } => {
                    info!(%name, %description, "sink added");

                    // if auto_link_new is set and this sink has no existing state entry, enable automatically
                    if config.auto_link_new {
                        state.sink_enabled.entry(name.clone()).or_insert(true);
                        if let Err(e) = state.save() {
                            warn!("failed to save state: {e:#}");
                        }
                    }

                    // if this sink is enabled in state and we have a default, send LinkSink command
                    if state.is_sink_enabled(&name) {
                        if current_default.as_deref() != Some(&name) {
                            let _ = cmd_tx.send(messages::PwCommand::LinkSink { name });
                        }
                    }
                }
                messages::PwEvent::SinkRemoved { name } => {
                    info!(%name, "sink removed");
                    // links will be torn down by pipewire automatically when node disappears
                }
                messages::PwEvent::DefaultChanged { name } => {
                    info!(%name, "default sink changed");
                    current_default = Some(name);
                }
            }
        }
    });

    pw_handle.join().expect("Pipewire thread panicked");
    Ok(())
}