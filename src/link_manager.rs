use pipewire::{core::Core, properties::properties};
use tracing::{debug, info, warn};

use crate::graph::{Graph, PortDirection};

/// Attempt to link the monitor ports of 'default_sink' to the playback ports of 'target_sink'.
/// 
/// Port pairs: monitor_FL -> playback_FL, monitor_FR -> playback_FR
/// 
/// Returns Ok(()) if both links were created, or an error describing why resolution or creation failed.

pub fn create_links(
    core: &Core,
    graph: &Graph,
    default_sink_name: &str,
    target_sink_name: &str,
) -> anyhow::Result<()> {
    let default_node = graph
        .node_by_name(default_sink_name)
        .ok_or_else(|| anyhow::anyhow!("default sink not found in graph: {default_sink_name}"))?;

    let target_node = graph
        .node_by_name(target_sink_name)
        .ok_or_else(|| anyhow::anyhow!("target sink not found in graph: {target_sink_name}"))?;

    // monitor port name on default, playback port name on target
    let port_pairs = [
        ("monitor_FL", "playback_FL"),
        ("monitor_FR", "playback_FR"),
    ];

    for (src_port_name, dst_port_name) in port_pairs {
        let src_port = graph
            .port_by_name(default_node.id, src_port_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "source port '{src_port_name}' not found on node '{default_sink_name}'"
                )
            })?;

        let dst_port = graph
            .port_by_name(target_node.id, dst_port_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dest port '{dst_port_name}' not found on node '{target_sink_name}'"
                )
            })?;
        
        info!(
            src = src_port_name,
            dst = dst_port_name,
            src_id = src_port.id,
            dst_id = dst_port.id,
            "creating link"
        );

        let props = properties! {
            "link.output.port" => src_port.id.to_string(),
            "link.input.port" => dst_port.id.to_string(),
            "link.output.node" => default_node.id.to_string(),
            "link.input.node" => target_node.id.to_string(),
            "object.linger" => "1",
        };

        core.create_object::<pipewire::link::Link>("link-factory", &props)
            .map_err(|e| anyhow::anyhow!("failed to create link: {e}"))?;

        debug!(src = src_port_name, dst = dst_port_name, "link created");
    }

    Ok(())
}

/// Destroy all active links whose output node is 'default_sink_name' and input node is 'target_sink_name'.

pub fn destroy_links(
    _graph: &Graph,
    default_sink_name: &str,
    target_sink_name: &str,
) {
    warn!(
        default = default_sink_name,
        target = target_sink_name,
        "destroy_links called - todo!"
    );
}