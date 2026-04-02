use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: u32,
    pub name: String,  // node.name - stable identifier
    pub description: String,  // node.description - human readable
    pub media_class: String,  // eg. "Audio/Sink", "Audio/Source"
}

#[derive(Debug, Clone)]
pub struct PortInfo {
    pub id: u32,
    pub node_id: u32,  // which node owns this port
    pub name: String,  // port.name eg. "monitor_FL", "playback_FR"
    pub direction: PortDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortDirection {
    Input,
    Output,
    Unknown,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<u32, NodeInfo>,
    pub ports: HashMap<u32, PortInfo>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, info: NodeInfo) {
        self.nodes.insert(info.id, info);
    }

    pub fn remove_node(&mut self, id: u32) {
        self.nodes.remove(&id);
        // also remove any ports that belonged to this node
        self.ports.retain(|_, p| p.node_id != id);
    }

    pub fn add_port(&mut self, info: PortInfo) {
        self.ports.insert(info.id, info);
    }

    pub fn remove_port(&mut self, id: u32) {
        self.ports.remove(&id);
    }

    /// Find a node by its stable node.name
    pub fn node_by_name(&self, name: &str) -> Option<&NodeInfo> {
        self.nodes.values().find(|n| n.name == name)
    }

    /// Find all ports belonging to a node, optionally filtered by direction
    pub fn ports_for_node(&self, node_id: u32, direction: Option<&PortDirection>) -> Vec<&PortInfo> {
        self.ports
            .values()
            .filter(|p| {
                p.node_id == node_id
                    && direction.map_or(true, |d| &p.direction == d)
            })
            .collect()
    }

    /// Find a specific port by node_id and port.name
    pub fn port_by_name(&self, node_id: u32, name: &str) -> Option<&PortInfo> {
        self.ports
            .values()
            .find(|p| p.node_id == node_id && p.name == name)
    }
}