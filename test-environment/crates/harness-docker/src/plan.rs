//! Pure description of the isolated Docker test cluster.
//!
//! A [`ClusterPlan`] is derived from a validated [`ClusterTopology`] plus the
//! [`PlanSettings`] network, images, and container environment: one shared
//! isolated network with one container per node. Construction cannot fail
//! because the topology has already been validated by [`harness_core`].

use std::collections::BTreeMap;
use std::net::SocketAddr;

use harness_core::{ClusterTopology, NodeId, NodeRole, NodeSpec};

/// Builds the environment entries for one node container.
pub type EnvBuilder = fn(&NodeSpec) -> BTreeMap<String, String>;

/// Name of the isolated Docker network the test cluster runs in.
#[derive(Clone)]
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
    /// Builds the environment entries for each node container.
    pub env: EnvBuilder,
    /// Container path of the node data volume that persists across container re-creation (TST-017); `None` adds no volume.
    pub data_volume_path: Option<String>,
    /// Command run after the image, overriding its default command when non-empty.
    pub command: Vec<String>,
    /// Linux capabilities granted to each container for packet filtering (TST-006).
    pub cap_add: Vec<String>,
}

impl PlanSettings {
    /// Creates settings from a network name, the two node images, the environment builder, and the data volume path.
    pub fn new(
        network: impl Into<String>,
        dedicated_server_image: impl Into<String>,
        storage_image: impl Into<String>,
        env: EnvBuilder,
        data_volume_path: Option<String>,
    ) -> Self {
        Self {
            network: network.into(),
            dedicated_server_image: dedicated_server_image.into(),
            storage_image: storage_image.into(),
            env,
            data_volume_path,
            command: Vec::new(),
            cap_add: Vec::new(),
        }
    }
    /// Sets the command run after each container's image.
    pub fn with_command(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
    /// Sets the capabilities granted to each container.
    pub fn with_cap_add(mut self, cap_add: Vec<String>) -> Self {
        self.cap_add = cap_add;
        self
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
#[derive(Clone)]
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
    /// Container path of the node data volume that persists across container re-creation (TST-017); `None` adds no volume.
    pub data_volume_path: Option<String>,
    /// Command run after the image, overriding its default command when non-empty.
    pub command: Vec<String>,
    /// Linux capabilities granted to the container for packet filtering (TST-006).
    pub cap_add: Vec<String>,
}

/// Complete description of the cluster: one network plus one container per node.
#[derive(Clone)]
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
                env: (settings.env)(spec),
                data_volume_path: settings.data_volume_path.clone(),
                command: settings.command.clone(),
                cap_add: settings.cap_add.clone(),
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
