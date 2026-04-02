use pipewire::{
    context::ContextRc,
    main_loop::MainLoopRc,
    metadata::Metadata,
    registry::GlobalObject,
    types::ObjectType
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use std::{cell::RefCell, rc::Rc};

use crate::{
    graph::{Graph, NodeInfo, PortDirection, PortInfo},
    messages::{PwCommand, PwEvent},
};

pub fn run(
    cmd_rx: std::sync::mpsc::Receiver<PwCommand>,
    event_tx: mpsc::UnboundedSender<PwEvent>,
) -> anyhow::Result<()> {

    pipewire::init();
    // The mainloop owns the PW event loop. Everything pipewire lives here
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;

    let registry_for_meta = core.get_registry_rc()?;

    // Graph lives on PW thread
    let graph = Rc::new(RefCell::new(Graph::new()));

    // std::sync::mpsc for cmd_rx as it needs to be polled from a non async context (pw main loop iteration callback)
    // event_tx is tokio unbounded because the tokio side consumes it async

    let graph_for_global = Rc::clone(&graph);
    let graph_for_remove = Rc::clone(&graph);
    let event_tx_for_global = event_tx.clone();

    // listener must stay alive - dropping it unregisters the callbacks
    let _listener = registry
        .add_listener_local()
        .global(move |global: &GlobalObject<&pipewire::spa::utils::dict::DictRef>| {
            let props = match global.props {
                Some(p) => p,
                None => return,
            };
            
            // global.type tells us what kind of object this is
            match global.type_ {
                ObjectType::Node => {
                    // props is a key-value bag, node.name is the stable identifier
                    let name = props.get("node.name").unwrap_or("").to_string();
                    let description = props.get("node.description").unwrap_or("").to_string();
                    let media_class = props.get("media.class").unwrap_or("").to_string();

                    info!(id = global.id, %name, %media_class, "node added");

                    graph_for_global.borrow_mut().add_node(NodeInfo {
                        id: global.id,
                        name: name.clone(),
                        description: description.clone(),
                        media_class: media_class.clone(),
                    });

                    // Only emit SinkAdded for actual audio sinks
                    if media_class == "Audio/Sink" {
                        let _ = event_tx_for_global.send(PwEvent::SinkAdded { name, description });
                    }
                }

                ObjectType::Port => {
                    let port_name = props.get("port.name").unwrap_or("").to_string();
                    let node_id: u32 = props
                        .get("node.id")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let direction = match props.get("port.direction") {
                        Some("in") => PortDirection::Input,
                        Some("out") => PortDirection::Output,
                        _ => PortDirection::Unknown,
                    };

                    debug!(id = global.id, %port_name, node_id, "port added");

                    graph_for_global.borrow_mut().add_port(PortInfo {
                        id: global.id,
                        node_id,
                        name: port_name,
                        direction,
                    });
                }

                ObjectType::Metadata => {
                    // Pipewire exposes the default sink via a Metadata object named "default". Bind to it and listen for property changes
                    let obj_name = props.get("metadata.name").unwrap_or("");
                    if obj_name != "default" {
                        return;
                    }

                    info!(id = global.id, "found default metadata object");

                    let event_tx_meta = event_tx_for_global.clone();

                    let metadata: Metadata = registry_for_meta
                        .bind(global)
                        .expect("failed to bind metadata object");

                    // property changes fire whenever the default sink/source changes
                    let _meta_listener = metadata
                        .add_listener_local()
                        .property(move |_subject, key, _type_, value| {
                            // key is "default.audio.sink" when the default sink changes
                            if key == Some("default.audio.sink") {
                                if let Some(val) = value {
                                    // value is a JSON string: {"name":"alsa_output..."}
                                    // parse out the name field simply
                                    if let Some(name) = parse_default_sink_name(val) {
                                        info!(%name, "default sink changed");
                                        let _ = event_tx_meta
                                            .send(PwEvent::DefaultChanged { name });
                                    }
                                }
                            }
                            // return 0 to keep listener alive
                            0
                        })
                        .register();

                    // leak metadata, listener so they stay alive for duration of main loop. Process owns them until exit
                    std::mem::forget(_meta_listener);
                    std::mem::forget(metadata);
                }

                _ => {
                    debug!(id = global.id, type_ = ?global.type_, "other object added");
                }
            }
        })
        .global_remove(move |id| {
            debug!(id, "object removed");
            let mut g = graph_for_remove.borrow_mut();

            // emit SinkRemoved if this id was and Audio/Sink node
            if let Some(node) = g.nodes.get(&id) {
                if node.media_class == "Audio/Sink" {
                    let _ = event_tx.send(PwEvent::SinkRemoved { name: node.name.clone() });
                }
            }

            g.remove_node(id);
            g.remove_port(id);
        })
        .register();

    info!("Pipewire registry listener ready - running main loop");

    // This blocks the thread, driving all PW callbacks until mainloop.quit() is called.
    mainloop.run();

    Ok(())
}

/// The default sink metadata value looks like: {"name":"alsa_output.pci-..."}
/// This is a minimal parser
fn parse_default_sink_name(value: &str) -> Option<String> {
    // find "name":"<value>" and extract value
    let key = "\"name\":\"";
    let start = value.find(key)? + key.len();
    let end = value[start..].find('"')? + start;
    Some(value[start..end].to_string())
}