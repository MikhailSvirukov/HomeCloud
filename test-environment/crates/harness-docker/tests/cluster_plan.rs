//! Docker-free integration tests over the `harness-docker` public API.
//!
//! These drive the plan and runtime seams exactly as an embedding scenario would:
//! build a validated topology, derive a [`ClusterPlan`], and exercise the in-memory
//! runtime. No Docker daemon or socket is touched.

use std::net::SocketAddr;

use harness_core::{ClusterTopology, NodeId, NodeRole, NodeSpec};
use harness_docker::{ClusterPlan, ClusterRuntime, FakeClusterRuntime, PlanSettings};

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
/// Network name configured for the isolated test cluster.
const NETWORK: &str = "harness-net";
/// Image configured for the dedicated-server container.
const SERVER_IMAGE: &str = "server:1";
/// Image configured for storage containers.
const STORAGE_IMAGE: &str = "storage:1";
/// Node id of the single dedicated server.
const DEDICATED_ID: u16 = 1;
/// First node id handed out to storage nodes.
const FIRST_STORAGE_ID: u16 = 2;
/// Base address port for storage node listen addresses.
const STORAGE_PORT_BASE: u16 = 7002;

/// Builds `127.0.0.1:<port>`.
fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// Builds a valid topology with one dedicated server and `storage` storage nodes.
fn topology_with_storage(storage: u16) -> ClusterTopology {
    let builder = ClusterTopology::builder().dedicated_server(NodeSpec::dedicated_server(
        NodeId::new(DEDICATED_ID),
        "dedicated",
        addr(7001),
    ));
    (0..storage)
        .fold(builder, |builder, i| {
            let id = i + FIRST_STORAGE_ID;
            builder.storage_node(NodeSpec::storage(
                NodeId::new(id),
                format!("s-{id}"),
                addr(STORAGE_PORT_BASE + i),
            ))
        })
        .build()
        .expect("one dedicated server plus any storage nodes is a valid topology")
}

/// Builds the plan settings used across these tests.
fn settings() -> PlanSettings {
    PlanSettings::new(NETWORK, SERVER_IMAGE, STORAGE_IMAGE)
}

/// Derives a plan for one dedicated server and `storage` storage nodes.
fn plan_with_storage(storage: u16) -> ClusterPlan {
    ClusterPlan::from_topology(&topology_with_storage(storage), &settings())
}

/// The node ids in plan (topology) order.
fn plan_order(plan: &ClusterPlan) -> Vec<NodeId> {
    plan.containers.iter().map(|c| c.node).collect()
}

#[test]
fn plan_places_dedicated_server_first_and_uses_role_images() {
    let plan = plan_with_storage(3);

    assert_eq!(
        plan.containers.len(),
        4,
        "1 dedicated + 3 storage => 4 containers"
    );

    let dedicated = plan
        .containers
        .first()
        .expect("a plan always has exactly one dedicated-server container first");
    assert_eq!(dedicated.node, NodeId::new(DEDICATED_ID));
    assert_eq!(dedicated.role, NodeRole::DedicatedServer);
    assert_eq!(dedicated.image, SERVER_IMAGE);

    for container in plan.containers.iter().skip(1) {
        assert_eq!(container.role, NodeRole::Storage);
        assert_eq!(container.image, STORAGE_IMAGE);
    }
}

#[test]
fn container_hostname_mirrors_node_label_and_plan_carries_network_name() {
    let plan = plan_with_storage(3);

    for container in &plan.containers {
        assert_eq!(
            container.hostname, container.label,
            "container hostname must equal its node label"
        );
    }

    assert_eq!(plan.network.name, NETWORK);
}

#[test]
fn container_environment_carries_node_identity_for_dedicated_and_storage() {
    let plan = plan_with_storage(3);

    let dedicated = plan
        .container(NodeId::new(DEDICATED_ID))
        .expect("dedicated-server container is present for id 1");
    assert_eq!(
        dedicated.env.get(ENV_NODE_ID).map(String::as_str),
        Some("1")
    );
    assert_eq!(
        dedicated.env.get(ENV_NODE_LABEL).map(String::as_str),
        Some("dedicated")
    );
    assert_eq!(
        dedicated.env.get(ENV_NODE_ROLE).map(String::as_str),
        Some(ROLE_DEDICATED_SERVER)
    );
    assert_eq!(
        dedicated.env.get(ENV_LISTEN_ADDRESS).map(String::as_str),
        Some("127.0.0.1:7001")
    );

    let storage = plan
        .container(NodeId::new(FIRST_STORAGE_ID))
        .expect("first storage container is present for id 2");
    assert_eq!(storage.env.get(ENV_NODE_ID).map(String::as_str), Some("2"));
    assert_eq!(
        storage.env.get(ENV_NODE_LABEL).map(String::as_str),
        Some("s-2")
    );
    assert_eq!(
        storage.env.get(ENV_NODE_ROLE).map(String::as_str),
        Some(ROLE_STORAGE)
    );
    assert_eq!(
        storage.env.get(ENV_LISTEN_ADDRESS).map(String::as_str),
        Some("127.0.0.1:7002")
    );
}

#[test]
fn fake_runtime_start_stop_round_trip_follows_plan_order_and_is_idempotent() {
    let plan = plan_with_storage(3);
    let mut runtime = FakeClusterRuntime::new();
    assert!(runtime.running_containers().is_empty());

    runtime.start(&plan).expect("in-memory start cannot fail");
    assert_eq!(runtime.running_containers(), plan_order(&plan));

    runtime.stop(&plan).expect("in-memory stop cannot fail");
    assert!(runtime.running_containers().is_empty());

    runtime.stop(&plan).expect("repeated stop is idempotent");
    runtime
        .remove(&plan)
        .expect("remove with nothing running is idempotent");
    assert!(runtime.running_containers().is_empty());

    runtime.start(&plan).expect("in-memory restart cannot fail");
    assert_eq!(runtime.running_containers(), plan_order(&plan));
}

#[test]
fn cluster_size_scales_container_count_and_container_lookup_finds_every_node() {
    for storage in [1_u16, 3, 5] {
        let topology = topology_with_storage(storage);
        let plan = ClusterPlan::from_topology(&topology, &settings());

        let expected = usize::from(storage) + 1;
        assert_eq!(
            plan.containers.len(),
            expected,
            "1 dedicated + {storage} storage => {expected} containers"
        );

        for spec in topology.nodes() {
            let container = plan
                .container(spec.id)
                .expect("every topology node id resolves to a container");
            assert_eq!(container.label, spec.label);
            assert_eq!(container.address, spec.address);
        }
    }
}
