use pipewire::{context::ContextBox, main_loop::MainLoopBox, registry::GlobalObject, types::ObjectType};
use tracing::{debug, info, warn};
use std::{cell::RefCell, rc::Rc};

use crate::graph::{Graph, NodeInfo, PortDirection, PortInfo};

pub fn run() -> anyhow::Result<()> {

    pipewire::init();
    // The mainloop owns the PW event loop. Everything pipewire lives here
    let mainloop = MainLoopBox::new(None)?;
    let context = ContextBox::new(&mainloop.loop_(), None)?;
    let core = context.connect(None)?;
    let registry = core.get_registry()?;

    // Graph lives on PW thread
    let graph = Rc::new(RefCell::new(Graph::new()));
    let graph_clone = Rc::clone(&graph);

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

                    graph_clone.borrow_mut().add_node(NodeInfo {
                        id: global.id,
                        name,
                        description,
                        media_class,
                    });
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

                    graph_clone.borrow_mut().add_port(PortInfo {
                        id: global.id,
                        node_id,
                        name: port_name,
                        direction,
                    });
                }

                _ => {
                    debug!(id = global.id, type_ = ?global.type_, "other object added");
                }
            }
        })
        .global_remove(move |id| {
            debug!(id, "object removed");
            // don't know the type here, try both. Only one will have the id
            graph.borrow_mut().remove_node(id);
            graph.borrow_mut().remove_port(id);
        })
        .register();

    info!("Pipewire registry listener ready - running main loop");

    // This blocks the thread, driving all PW callbacks until mainloop.quit() is called.
    mainloop.run();

    Ok(())
}