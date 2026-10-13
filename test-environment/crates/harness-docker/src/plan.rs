//! Pure description of the isolated Docker test cluster.
//!
//! A [`ClusterPlan`] is derived from a validated [`ClusterTopology`] plus the
//! [`PlanSettings`] network and images: one shared isolated network with one
//! container per node. Construction cannot fail because the topology has
//! already been validated by [`harness_core`].

use std::collections::BTreeMap;
use std::net::SocketAddr;

use harness_core::{ClusterTopology, NodeId, NodeRole, NodeSpec};

/// Environment key carrying the node's numeric identifier.
const ENV_NODE_ID: &str = "NODE_ID";
/// Environment key carrying the node's unique label.
const ENV_NODE_LABEL: &str = "NODE_LABEL";
/// Environment key carrying the node's role.
const ENV_NODE_ROLE: &str = "NODE_ROLE";
/// Environment key carrying the node's listen address.
const ENV_LISTEN_ADDRESS: &str = "LISTEN_ADDRESS";
/// Environment value used for the dedicated-server role.
const ROLE_DEDICATED_SERVER: &str = "dedicated-server";
/// Environment value used for the storage role.
const ROLE_STORAGE: &str = "storage";

/// Name of the isolated Docker network the test cluster runs in.
pub struct NetworkPlan {
    /// The network name.
    pub name: String,
}

impl NetworkPlan {
    /// Creates a network plan with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Images and network used to build a [`ClusterPlan`].
pub struct PlanSettings {
    /// The target network name.
    pub network: String,
    /// Image used for the dedicated server container.
    pub dedicated_server_image: String,
    /// Image used for storage node containers.
    pub storage_image: String,
}

impl PlanSettings {
    /// Creates settings from a network name and the two node images.
    pub fn new(
        network: impl Into<String>,
        dedicated_server_image: impl Into<String>,
        storage_image: impl Into<String>,
    ) -> Self {
        Self {
            network: network.into(),
            dedicated_server_image: dedicated_server_image.into(),
            storage_image: storage_image.into(),
        }
    }
    /// Image for a node role.
    fn image_for(&self, role: NodeRole) -> &str {
        match role {
            NodeRole::DedicatedServer => &self.dedicated_server_image,
            NodeRole::Storage => &self.storage_image,
        }
    }
}

/// One node container in the isolated network.
pub struct ContainerPlan {
    /// Identifier of the node this container runs.
    pub node: NodeId,
    /// Role the node plays in the cluster.
    pub role: NodeRole,
    /// Human-readable label, unique within the topology.
    pub label: String,
    /// Container image the node runs.
    pub image: String,
    /// Container hostname on the isolated network.
    pub hostname: String,
    /// Address the container listens on inside the network.
    pub address: SocketAddr,
    /// Environment entries passed to the container.
    pub env: BTreeMap<String, String>,
}

/// Complete description of the cluster: one network plus one container per node.
pub struct ClusterPlan {
    /// The shared isolated network.
    pub network: NetworkPlan,
    /// Containers in topology order (dedicated server first, then storage).
    pub containers: Vec<ContainerPlan>,
}

impl ClusterPlan {
    /// Derives a plan from a validated topology and settings, dedicated server first.
    pub fn from_topology(topology: &ClusterTopology, settings: &PlanSettings) -> Self {
        let containers = topology
            .nodes()
            .map(|spec| ContainerPlan {
                node: spec.id,
                role: spec.role,
                label: spec.label.clone(),
                image: settings.image_for(spec.role).to_string(),
                hostname: spec.label.clone(),
                address: spec.address,
                env: env_for(spec),
            })
            .collect();
        Self {
            network: NetworkPlan::new(settings.network.clone()),
            containers,
        }
    }
    /// Looks up a container by node id.
    pub fn container(&self, node: NodeId) -> Option<&ContainerPlan> {
        self.containers
            .iter()
            .find(|container| container.node == node)
    }
}

/// Environment value for a node role.
fn role_value(role: NodeRole) -> &'static str {
    match role {
        NodeRole::DedicatedServer => ROLE_DEDICATED_SERVER,
        NodeRole::Storage => ROLE_STORAGE,
    }
}

/// Builds the four fixed environment entries for a node spec.
fn env_for(spec: &NodeSpec) -> BTreeMap<String, String> {
    BTreeMap::from([
        (ENV_NODE_ID.to_string(), spec.id.0.to_string()),
        (ENV_NODE_LABEL.to_string(), spec.label.clone()),
        (ENV_NODE_ROLE.to_string(), role_value(spec.role).to_string()),
        (ENV_LISTEN_ADDRESS.to_string(), spec.address.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    fn topology(n: u16) -> ClusterTopology {
        let mut builder = ClusterTopology::builder().dedicated_server(NodeSpec::dedicated_server(
            NodeId::new(1),
            "dedicated",
            addr(7001),
        ));
        for i in 0..n {
            let id = i + 2;
            builder = builder.storage_node(NodeSpec::storage(
                NodeId::new(id),
                format!("s-{id}"),
                addr(7002 + i),
            ));
        }
        builder.build().expect("valid topology")
    }

    fn settings() -> PlanSettings {
        PlanSettings::new("harness-net", "server:1", "storage:1")
    }

    #[test]
    fn plan_for_one_dedicated_and_three_storage_has_four_containers() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        assert_eq!(plan.containers.len(), 4);
    }

    #[test]
    fn dedicated_server_is_first_and_uses_dedicated_image() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        let first = plan.containers.first().expect("at least one container");
        assert_eq!(first.node, NodeId::new(1));
        assert_eq!(first.role, NodeRole::DedicatedServer);
        assert_eq!(first.image, "server:1");
    }

    #[test]
    fn storage_containers_use_storage_image() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        for container in plan.containers.iter().skip(1) {
            assert_eq!(container.role, NodeRole::Storage);
            assert_eq!(container.image, "storage:1");
        }
    }

    #[test]
    fn hostname_equals_label() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        for container in &plan.containers {
            assert_eq!(container.hostname, container.label);
        }
    }

    #[test]
    fn every_container_sets_all_four_env_keys() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        for container in &plan.containers {
            assert_eq!(container.env.len(), 4);
            assert_eq!(
                container.env.get(ENV_NODE_ID),
                Some(&container.node.0.to_string())
            );
            assert_eq!(container.env.get(ENV_NODE_LABEL), Some(&container.label));
            assert_eq!(
                container.env.get(ENV_LISTEN_ADDRESS),
                Some(&container.address.to_string())
            );
        }
    }

    #[test]
    fn env_role_reflects_node_role() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        let dedicated = plan.containers.first().expect("at least one container");
        assert_eq!(
            dedicated.env.get(ENV_NODE_ROLE).map(String::as_str),
            Some(ROLE_DEDICATED_SERVER)
        );
        for container in plan.containers.iter().skip(1) {
            assert_eq!(
                container.env.get(ENV_NODE_ROLE).map(String::as_str),
                Some(ROLE_STORAGE)
            );
        }
    }

    #[test]
    fn container_lookup_hits_known_id() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        assert_eq!(
            plan.container(NodeId::new(3)).map(|c| c.label.as_str()),
            Some("s-3")
        );
    }

    #[test]
    fn container_lookup_misses_unknown_id() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        assert!(plan.container(NodeId::new(99)).is_none());
    }

    #[test]
    fn network_name_is_carried_through() {
        let plan = ClusterPlan::from_topology(&topology(3), &settings());
        assert_eq!(plan.network.name, "harness-net");
    }

    #[test]
    fn container_count_tracks_storage_count() {
        for n in [1, 3, 5] {
            let plan = ClusterPlan::from_topology(&topology(n), &settings());
            assert_eq!(plan.containers.len(), usize::from(n) + 1);
        }
    }
}
