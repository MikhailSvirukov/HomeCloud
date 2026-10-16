//! Shared fixture for the `harness-docker` integration tests.
//!
//! Included with `mod common;` from each integration test target through the
//! standard Cargo shared-test-module pattern; it is not a test target itself.
//! It holds only the plan fixture reused by several tests, never file-specific
//! helpers, and every item is `pub` so the test modules can import what they use.

// Each test target uses a different subset of this module, so unused items in a
// given target are expected rather than dead code.
#![allow(dead_code)]

use std::collections::BTreeMap;

use harness_core::{ClusterTopology, NodeId, NodeRole, NodeSpec};
use harness_docker::{ClusterPlan, EnvBuilder, PlanSettings};
use harness_test_support::topology;

/// Network name configured for the isolated test cluster.
pub const NETWORK_NAME: &str = "harness-net";
/// Image configured for the dedicated-server container.
pub const SERVER_IMAGE: &str = "server:1";
/// Image configured for storage containers.
pub const STORAGE_IMAGE: &str = "storage:1";
/// Node id of the single dedicated server.
pub const DEDICATED_ID: u16 = 1;
/// First node id handed out to storage nodes.
pub const FIRST_STORAGE_ID: u16 = 2;
/// Number of storage nodes in the shared fixture.
pub const STORAGE_NODES: u16 = 3;

/// Environment key holding the node id in the shared settings.
pub const NODE_ID_KEY: &str = "NODE_ID";
/// Environment key holding the node label in the shared settings.
pub const NODE_LABEL_KEY: &str = "NODE_LABEL";
/// Environment key holding the node role in the shared settings.
pub const NODE_ROLE_KEY: &str = "NODE_ROLE";
/// Environment key holding the listen address in the shared settings.
pub const LISTEN_ADDRESS_KEY: &str = "LISTEN_ADDRESS";
/// Role value rendered for the dedicated server.
pub const DEDICATED_SERVER_ROLE: &str = "dedicated-server";
/// Role value rendered for storage nodes.
pub const STORAGE_ROLE: &str = "storage";

/// Role value the shared settings builder renders for a role.
pub fn role_value(role: NodeRole) -> &'static str {
    match role {
        NodeRole::DedicatedServer => DEDICATED_SERVER_ROLE,
        NodeRole::Storage => STORAGE_ROLE,
    }
}

/// Environment builder the shared settings use, matching the caller contract.
pub fn node_env(spec: &NodeSpec) -> BTreeMap<String, String> {
    BTreeMap::from([
        (NODE_ID_KEY.to_string(), spec.id.to_string()),
        (NODE_LABEL_KEY.to_string(), spec.label.clone()),
        (NODE_ROLE_KEY.to_string(), role_value(spec.role).to_string()),
        (LISTEN_ADDRESS_KEY.to_string(), spec.address.to_string()),
    ])
}

/// Builds the plan settings used across these tests.
pub fn settings() -> PlanSettings {
    let env: EnvBuilder = node_env;
    PlanSettings::new(NETWORK_NAME, SERVER_IMAGE, STORAGE_IMAGE, env)
}

/// Builds the shared fixture topology for `storage` storage nodes.
pub fn topology_with_storage(storage: u16) -> ClusterTopology {
    topology(storage)
}

/// Derives a plan for one dedicated server and the default storage count.
pub fn plan() -> ClusterPlan {
    plan_with_storage(STORAGE_NODES)
}

/// Derives a plan for one dedicated server and `storage` storage nodes.
pub fn plan_with_storage(storage: u16) -> ClusterPlan {
    ClusterPlan::from_topology(&topology(storage), &settings())
}

/// The node ids in plan (topology) order.
pub fn plan_order(plan: &ClusterPlan) -> Vec<NodeId> {
    plan.containers.iter().map(|c| c.node).collect()
}
