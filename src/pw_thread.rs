use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use pipewire::{
    context::ContextRc,
    main_loop::MainLoopRc,
    metadata::Metadata,
    registry::GlobalObject,
    types::ObjectType
};
use tokio::sync::mpsc;
use tracing::{debug, info, warn, error};

use crate::{
    graph::{NodeInfo, PortDirection, PortInfo},
    messages::{PwCommand, PwEvent},
};

pub fn run(
    cmd_rx: pipewire::channel::Receiver<PwCommand>,
    event_tx: mpsc::UnboundedSender<PwEvent>,
    graph: std::sync::Arc<std::sync::Mutex<crate::graph::Graph>>,
    default_sink: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) -> anyhow::Result<()> {
    // The mainloop owns the PW event loop. Everything pipewire lives here
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let registry_for_meta = core.get_registry_rc()?;
    let registry_for_bind = registry.clone();
    
    // std::sync::mpsc for cmd_rx as it needs to be polled from a non async context (pw main loop iteration callback)
    // event_tx is tokio unbounded because the tokio side consumes it async
    let event_tx_for_global = event_tx.clone();
    let default_sink_name_for_registry = Arc::clone(&default_sink);
    let default_sink_name_for_cmd = Arc::clone(&default_sink);

    let core_for_cmd = core.clone();
    let mainloop_for_cmd = mainloop.clone();

    // map to hold active links, key = target sink name
    let active_links = Rc::new(RefCell::new(HashMap::<String, Vec<pipewire::link::Link>>::new()));
    let active_links_for_cmd = Rc::clone(&active_links);
    let node_proxies = Rc::new(RefCell::new(HashMap::<u32, pipewire::node::Node>::new()));
    let node_proxies_for_registry = Rc::clone(&node_proxies);
    let node_proxies_for_remove = Rc::clone(&node_proxies);
    
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

                    let node = NodeInfo {
                        id: global.id,
                        name: name.clone(),
                        description: description.clone(),
                        media_class: media_class.clone(),
                    };

                    event_tx_for_global.send(PwEvent::NodeAdded(node)).ok();

                    let node_proxy: pipewire::node::Node = registry_for_bind
                        .bind(global)
                        .expect("Failed to bind node object");
                    node_proxies_for_registry.borrow_mut().insert(global.id, node_proxy);

                    // Only emit SinkAdded for actual audio sinks
                    if media_class == "Audio/Sink" {
                        event_tx_for_global.send(PwEvent::SinkAdded { name, description }).ok();
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

                    event_tx_for_global.send(PwEvent::PortAdded(PortInfo {
                        id: global.id,
                        node_id,
                        name: port_name,
                        direction,
                    })).ok();
                }

                ObjectType::Metadata => {
                    // Pipewire exposes the default sink via a Metadata object named "default". Bind to it and listen for property changes
                    let obj_name = props.get("metadata.name").unwrap_or("");
                    if obj_name != "default" {
                        return;
                    }

                    info!(id = global.id, "found default metadata object");

                    let event_tx_meta = event_tx_for_global.clone();
                    let default_sink_name_for_meta = Arc::clone(&default_sink_name_for_registry);

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
                                        *default_sink_name_for_meta.lock().unwrap() = Some(name.clone());
                                        info!(%name, "default sink changed");
                                        event_tx_meta.send(PwEvent::DefaultChanged { name }).ok();
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
            node_proxies_for_remove.borrow_mut().remove(&id);
            event_tx.send(PwEvent::NodeRemoved(id)).ok();
            event_tx.send(PwEvent::PortRemoved(id)).ok();
        })
        .register();

    // wakes up pipewire loop whenever tokio sends a command
    let _receiver = cmd_rx.attach(mainloop.loop_(), move |cmd| {
        match cmd {
            PwCommand::LinkSink { name } => {
                let default_name = default_sink_name_for_cmd.lock().unwrap().clone();
                if let Some(def_name) = default_name {
                    let g = graph.lock().unwrap();
                    
                    match crate::link_manager::create_links(&core_for_cmd, &g, &def_name, &name) {
                        Ok(links) => {
                            active_links_for_cmd.borrow_mut().insert(name.clone(), links);
                            info!("Successfully linked and stored: {name}");
                        }
                        Err(e) => error!("Failed to create links: {e:#}"),
                    }
                } else {
                    warn!("Cannot link '{name}', no default sink known yet");
                }
            }
            PwCommand::UnlinkSink { name } => {
                if active_links_for_cmd.borrow_mut().remove(&name).is_some() {
                    info!("Destroyed links for {name}");
                }
            }
            PwCommand::SetVolume { node_id, volume } => {
                // apply_volume(&node_proxies_for_cmd, node_id, volume);
                match std::process::Command::new("wpctl")
                    .arg("set-volume")
                    .arg(node_id.to_string())
                    .arg(volume.to_string())
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        info!(node_id, volume, "volume applied via WirePlumber");
                    }
                    Ok(output) => {
                        let err = String::from_utf8_lossy(&output.stderr);
                        error!(node_id, "wpctl failed to set volume: {}", err.trim());
                    }
                    Err(e) => {
                        error!(node_id, "failed to execute wpctl (is WirePlumber installed?): {}", e);
                    }
                }
            }
            PwCommand::Quit => {
                mainloop_for_cmd.quit();
            }
        }
    });

    // We must leak the receiver so it isn't dropped at the end of this scope keeping the callback alive for the duration of the machine
    std::mem::forget(_receiver);

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