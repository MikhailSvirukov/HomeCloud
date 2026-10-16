//! Pure description of the isolated Docker test cluster.
//!
//! A [`ClusterPlan`] is derived from a validated [`ClusterTopology`] plus the
//! [`PlanSettings`] network, images, and container environment: one shared
//! isolated network with one container per node. Construction cannot fail
//! because the topology has already been validated by [`harness_core`].

use std::collections::BTreeMap;
use std::net::SocketAddr;

use harness_core::{ClusterTopology, NodeId, NodeRole, NodeSpec};

/// Environment contract a plan uses when it renders node containers.
///
/// The four keys and two role values must all be distinct: they share one
/// [`BTreeMap`] key space, so a duplicate would silently drop an entry.
pub struct ContainerEnv {
    /// Environment key holding the node id.
    pub node_id: String,
    /// Environment key holding the node label.
    pub node_label: String,
    /// Environment key holding the node role.
    pub node_role: String,
    /// Environment key holding the listen address.
    pub listen_address: String,
    /// Role value used for the dedicated server.
    pub dedicated_server_role: String,
    /// Role value used for storage nodes.
    pub storage_role: String,
}

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

/// Images, network, and environment used to build a [`ClusterPlan`].
pub struct PlanSettings {
    /// The target network name.
    pub network: String,
    /// Image used for the dedicated server container.
    pub dedicated_server_image: String,
    /// Image used for storage node containers.
    pub storage_image: String,
    /// Environment contract for the node containers.
    pub env: ContainerEnv,
}

impl PlanSettings {
    /// Creates settings from a network name, the two node images, and the environment.
    pub fn new(
        network: impl Into<String>,
        dedicated_server_image: impl Into<String>,
        storage_image: impl Into<String>,
        env: ContainerEnv,
    ) -> Self {
        Self {
            network: network.into(),
            dedicated_server_image: dedicated_server_image.into(),
            storage_image: storage_image.into(),
            env,
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
                env: env_for(spec, &settings.env),
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
fn role_value(env: &ContainerEnv, role: NodeRole) -> &str {
    match role {
        NodeRole::DedicatedServer => &env.dedicated_server_role,
        NodeRole::Storage => &env.storage_role,
    }
}

/// Builds the four fixed environment entries for a node spec.
fn env_for(spec: &NodeSpec, env: &ContainerEnv) -> BTreeMap<String, String> {
    BTreeMap::from([
        (env.node_id.clone(), spec.id.0.to_string()),
        (env.node_label.clone(), spec.label.clone()),
        (
            env.node_role.clone(),
            role_value(env, spec.role).to_string(),
        ),
        (env.listen_address.clone(), spec.address.to_string()),
    ])
}
