use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::info;
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
    pub default_sink: Arc<Mutex<Option<String>>>,
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
        let current_default = self.default_sink.lock().unwrap().clone();
        
        graph
            .nodes
            .values()
            .filter(|n| n.media_class == "Audio/Sink")
            .map(|n| SinkInfo {
                name: n.name.clone(),
                description: n.description.clone(),
                is_default: Some(n.name.clone()) == current_default,
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

    async fn set_sink_volume(
        &self,
        name: String,
        volume: f32,
    ) -> zbus::fdo::Result<()> {
        // Clamp volume 0.0 and 1.0
        let clamped = volume.clamp(0.0, 1.0);
        let node_id = {
            let graph = self.graph.lock().unwrap();
            graph
                .node_by_name(&name)
                .map(|n| n.id)
                .ok_or_else(|| {
                    zbus::fdo::Error::Failed(format!("sink not found in graph: {name}"))
                })?
        };

        self.cmd_tx
            .send(PwCommand::SetVolume {
            node_id,
            volume: clamped
        }).ok();

        info!(%name, node_id, volume = clamped, "volume changed via D-Bus");
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

    async fn set_default_sink(&self, name: String) -> zbus::fdo::Result<()> {
        self.cmd_tx
            .send(PwCommand::SetDefaultSink { name: name.clone() })
            .ok();
        info!(%name, "default sink change requested via D-Bus");
        Ok(())
    }

    fn get_sink_volume(&self, name: String) -> zbus::fdo::Result<f64> {
        let node_id = {
            let graph = self.graph.lock().unwrap();
            graph
                .node_by_name(&name)
                .map(|n| n.id)
                .ok_or_else(|| zbus::fdo::Error::Failed(format!("sink not found: {name}")))?
        };

        let output = std::process::Command::new("wpctl")
            .arg("get-volume")
            .arg(node_id.to_string())
            .output()
            .map_err(|e| zbus::fdo::Error::Failed(format!("wpctl failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let volume = stdout
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);

        Ok(volume)
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
    default_sink: Arc<Mutex<Option<String>>>,
    cmd_tx: pipewire::channel::Sender<PwCommand>,
    mut event_rx: mpsc::UnboundedReceiver<PwEvent>,
) -> anyhow::Result<()> {
    let iface = PatchwireInterface {
        state,
        config,
        graph: graph.clone(),
        default_sink,
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