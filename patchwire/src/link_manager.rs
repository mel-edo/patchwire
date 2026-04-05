use pipewire::{core::Core, properties::properties};
use tracing::{debug, info};

use crate::graph::Graph;

/// Attempt to link the monitor ports of 'default_sink' to the playback ports of 'target_sink'.
/// Port pairs: monitor_FL -> playback_FL, monitor_FR -> playback_FR
/// Returns a vec of created pipewire links. When these objects are dropped, the links are automatically destroyed in pipewire.

pub fn create_links(
    core: &Core,
    graph: &Graph,
    default_sink_name: &str,
    target_sink_name: &str,
) -> anyhow::Result<Vec<pipewire::link::Link>> {
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

    let mut created_links = Vec::new();

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
        };

        let link = core.create_object::<pipewire::link::Link>("link-factory", &props)
            .map_err(|e| anyhow::anyhow!("failed to create link: {e}"))?;

        created_links.push(link);
        debug!(src = src_port_name, dst = dst_port_name, "link created");
    }

    Ok(created_links)
}