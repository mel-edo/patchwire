use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, error};
use zbus::{connection, interface, object_server::SignalEmitter, zvariant::Type};

use crate::{
    config::Config,
    graph::Graph,
    messages::{PwCommand, PwEvent},
    state::State,
};

/// The data every D-Bus method handler needs access to
/// Sits behind Arc<Mutex<>> so it can be shared between the interface methods (called by zbus on incoming D-Bus calls) and
/// the event loop (which updates state when PW events arrive)
pub struct PatchwireInterface {
    pub state: Arc<Mutex<State>>,
    pub config: Arc<Mutex<Config>>,
    pub graph: Arc<Mutex<Graph>>,
    pub cmd_tx: pipewire::channel::Sender<PwCommand>,
}

/// SinkInfo is the struct returned by ListSinks over D-Bus
/// zbus will serialize this automatically via serde
#[derive(Debug, Serialize, Deserialize, Type)]
pub struct SinkInfo {
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub is_linked: bool,
    pub is_enabled: bool,
}

#[interface(name = "com.patchwire.Daemon")]
impl PatchwireInterface {
    /// List all known audio sinks with their current state.
    fn list_sinks(&self) -> Vec<SinkInfo> {
        let graph = self.graph.lock().unwrap();
        let state = self.state.lock().unwrap();
        let config = self.config.lock().unwrap();

        let default_name = config.active_profile
            .as_deref()
            .unwrap_or("");
        
        graph
            .nodes
            .values()
            .filter(|n| n.media_class == "Audio/Sink")
            .map(|n| SinkInfo {
                name: n.name.clone(),
                description: n.description.clone(),
                is_default: n.name == default_name,
                is_linked: false,
                is_enabled: state.is_sink_enabled(&n.name),
            })
            .collect()
    }

    /// Enable or disable linking for a single sink
    async fn set_sink_enabled(
        &self,
        name: String,
        enabled: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.set_sink_enabled(&name, enabled).map_err(|e| {
                zbus::fdo::Error::Failed(format!("failed to save state: {e:#}"))
            })?;
        }

        let cmd = if enabled {
            PwCommand::LinkSink { name: name.clone() }
        } else {
            PwCommand::UnlinkSink { name: name.clone() }
        };

        self.cmd_tx.send(cmd).ok();
        
        Self::link_state_changed(&emitter, &name, enabled).await.ok();
        info!(%name, enabled, "sink toggled via D-Bus");
        Ok(())
    }

    /// Return all saved profile names
    fn get_profiles(&self) -> Vec<String> {
        self.config.lock().unwrap().profiles.keys().cloned().collect()
    }

    /// Switch to a saved profile - updates state and sends LinkSink/UnlinkSink for each sink in the graph
    async fn set_active_profile(
        &self,
        name: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let sinks_to_enable: Vec<String>;
        let all_sink_names: Vec<String>;

        {
            let mut config = self.config.lock().unwrap();

            let profile = config.profiles.get(&name).ok_or_else(|| {
                zbus::fdo::Error::Failed(format!("profile not found: {name}"))
            })?;

            sinks_to_enable = profile.enabled_sinks.clone();
            config.active_profile = Some(name.clone());

            // Persist the active_profile change
            if let Err(e) = config.save() {
                error!("Failed to save config: {e:#}");
            }
        }

        {
            let graph = self.graph.lock().unwrap();
            all_sink_names = graph
                .nodes
                .values()
                .filter(|n| n.media_class == "Audio/Sink")
                .map(|n| n.name.clone())
                .collect();
        }

        // Apply the profile: enable sinks listed in it, disable the rest
        {
            let mut state = self.state.lock().unwrap();
            for sink_name in &all_sink_names {
                let enabled = sinks_to_enable.contains(sink_name);
                state.set_sink_enabled(sink_name, enabled).map_err(|e| {
                    zbus::fdo::Error::Failed(format!("failed to save state: {e:#}"))
                })?;

                let cmd = if enabled {
                    PwCommand::LinkSink { name: sink_name.clone() }
                } else {
                    PwCommand::UnlinkSink { name: sink_name.clone() }
                };
                self.cmd_tx.send(cmd).ok();
            }
        }

        Self::sinks_changed(&emitter).await.ok();
        info!(%name, "profile activated via D-Bus");
        Ok(())
    }

    /// Save current state as a named profile
    fn save_profile(&self, name: String) -> zbus::fdo::Result<()> {
        let state = self.state.lock().unwrap();
        let mut config = self.config.lock().unwrap();

        let enabled_sinks: Vec<String> = state
            .sink_enabled
            .iter()
            .filter_map(|(k, v)| if *v { Some(k.clone()) } else { None })
            .collect();

        config.profiles.insert(name.clone(), crate::config::Profile {
            description: None,
            enabled_sinks,
        });

        // Persist the new profile
        if let Err(e) = config.save() {
            error!("Failed to save config: {e:#}");
        }

        info!(%name, "profile saved via D-Bus");
        Ok(())
    }

    /// Delete a saved profile
    fn delete_profile(&self, name: String) -> zbus::fdo::Result<()> {
        let mut config = self.config.lock().unwrap();
        if config.profiles.remove(&name).is_none() {
            return Err(zbus::fdo::Error::Failed(format!("profile not found: {name}")));
        }

        if let Err(e) = config.save() {
            error!("Failed to save config after deleting profile: {e:#}");
        }
        info!(%name, "profile deleted via D-Bus");
        Ok(())
    }

    /// Return the current default sink name
    fn get_default_sink(&self) -> String {
        self.config
            .lock()
            .unwrap()
            .active_profile
            .clone()
            .unwrap_or_default()
    }

    // Signals

    /// Fired when a sink is added or removed from the graph
    #[zbus(signal)]
    async fn sinks_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Fired when the default sink changes
    #[zbus(signal)]
    async fn default_changed(emitter: &SignalEmitter<'_>, new_default: &str) -> zbus::Result<()>;

    /// Fired when a link is created or destroyed for a sink
    #[zbus(signal)]
    async fn link_state_changed(
        emitter: &SignalEmitter<'_>,
        name: &str,
        linked: bool,
    ) -> zbus::Result<()>;
}

/// Start the D-Bus server and drive the PwEvent loop
/// This is spawned as a tokio task from main.rs
pub async fn run(
    state: Arc<Mutex<State>>,
    config: Arc<Mutex<Config>>,
    graph: Arc<Mutex<Graph>>,
    cmd_tx: pipewire::channel::Sender<PwCommand>,
    mut event_rx: mpsc::UnboundedReceiver<PwEvent>,
) -> anyhow::Result<()> {
    let iface = PatchwireInterface {
        state,
        config,
        graph: graph.clone(),
        cmd_tx,
    };

    let conn = connection::Builder::session()?
        .name("com.patchwire.Daemon")?
        .serve_at("/com/patchwire/Daemon", iface)?
        .build()
        .await?;

    info!("D-Bus interface registered at com.patchwire.Daemon");

    // Get a signal emitter we can use from outside the interface methods
    let emitter = conn
        .object_server()
        .interface::<_, PatchwireInterface>("/com/patchwire/Daemon")
        .await?
        .signal_emitter()
        .clone();

    // Drive PwEvents - update shared state and emit D-Bus signals
    while let Some(event) = event_rx.recv().await {
        match event {
            PwEvent::NodeAdded(node) => {
                graph.lock().unwrap().add_node(node);
            }

            PwEvent::NodeRemoved(id) => {
                graph.lock().unwrap().remove_node(id);
            }

            PwEvent::PortAdded(port) => {
                graph.lock().unwrap().add_port(port);
            }

            PwEvent::PortRemoved(id) => {
                graph.lock().unwrap().remove_port(id);
            }

            PwEvent::SinkAdded { name, description } => {
                info!(%name, %description, "sink added");
                PatchwireInterface::sinks_changed(&emitter).await.ok();
            }

            PwEvent::SinkRemoved { name } => {
                info!(%name, "sink removed");
                PatchwireInterface::sinks_changed(&emitter).await.ok();
            }

            PwEvent::DefaultChanged { name } => {
                info!(%name, "default sink changed");
                PatchwireInterface::default_changed(&emitter, &name).await.ok();
            }
        }
    }
    Ok(())
}