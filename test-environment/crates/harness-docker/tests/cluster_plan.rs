//! Docker-free integration tests over the `harness-docker` public API.
//!
//! These drive the plan and runtime seams exactly as an embedding scenario would:
//! build a validated topology, derive a [`ClusterPlan`], and exercise the in-memory
//! runtime. No Docker daemon or socket is touched.

use harness_core::{NodeId, NodeRole};
use harness_docker::{ClusterPlan, ClusterRuntime, ContainerEnv, FakeClusterRuntime, PlanSettings};
use harness_test_support::{addr, topology};

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
/// Number of storage nodes in the shared fixture.
const STORAGE_NODES: u16 = 3;

/// Builds the plan settings used across these tests.
fn settings() -> PlanSettings {
    PlanSettings::new(
        NETWORK,
        SERVER_IMAGE,
        STORAGE_IMAGE,
        ContainerEnv {
            node_id: "NODE_ID".to_string(),
            node_label: "NODE_LABEL".to_string(),
            node_role: "NODE_ROLE".to_string(),
            listen_address: "LISTEN_ADDRESS".to_string(),
            dedicated_server_role: "dedicated-server".to_string(),
            storage_role: "storage".to_string(),
        },
    )
}

/// Derives a plan for one dedicated server and `storage` storage nodes.
fn plan_with_storage(storage: u16) -> ClusterPlan {
    ClusterPlan::from_topology(&topology(storage), &settings())
}

/// The node ids in plan (topology) order.
fn plan_order(plan: &ClusterPlan) -> Vec<NodeId> {
    plan.containers.iter().map(|c| c.node).collect()
}

#[test]
fn plan_places_dedicated_server_first_and_uses_role_images() {
    let plan = plan_with_storage(STORAGE_NODES);

    assert_eq!(
        plan.containers.len(),
        usize::from(STORAGE_NODES) + 1,
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
    let plan = plan_with_storage(STORAGE_NODES);

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
    let plan = plan_with_storage(STORAGE_NODES);

    let dedicated = plan
        .container(NodeId::new(DEDICATED_ID))
        .expect("dedicated-server container is present for id 1");
    assert_eq!(dedicated.env.get("NODE_ID").map(String::as_str), Some("1"));
    assert_eq!(
        dedicated.env.get("NODE_LABEL").map(String::as_str),
        Some("dedicated")
    );
    assert_eq!(
        dedicated.env.get("NODE_ROLE").map(String::as_str),
        Some("dedicated-server")
    );
    let dedicated_address = addr(DEDICATED_ID).to_string();
    assert_eq!(
        dedicated.env.get("LISTEN_ADDRESS").map(String::as_str),
        Some(dedicated_address.as_str())
    );

    let storage = plan
        .container(NodeId::new(FIRST_STORAGE_ID))
        .expect("first storage container is present for id 2");
    assert_eq!(storage.env.get("NODE_ID").map(String::as_str), Some("2"));
    assert_eq!(
        storage.env.get("NODE_LABEL").map(String::as_str),
        Some("storage-1")
    );
    assert_eq!(
        storage.env.get("NODE_ROLE").map(String::as_str),
        Some("storage")
    );
    let storage_address = addr(FIRST_STORAGE_ID).to_string();
    assert_eq!(
        storage.env.get("LISTEN_ADDRESS").map(String::as_str),
        Some(storage_address.as_str())
    );
}

#[test]
fn fake_runtime_start_stop_round_trip_follows_plan_order_and_is_idempotent() {
    let plan = plan_with_storage(STORAGE_NODES);
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
        let cluster = topology(storage);
        let plan = ClusterPlan::from_topology(&cluster, &settings());

        let expected = usize::from(storage) + 1;
        assert_eq!(
            plan.containers.len(),
            expected,
            "1 dedicated + {storage} storage => {expected} containers"
        );

        for spec in cluster.nodes() {
            let container = plan
                .container(spec.id)
                .expect("every topology node id resolves to a container");
            assert_eq!(container.label, spec.label);
            assert_eq!(container.address, spec.address);
        }
    }
}

#[test]
fn container_lookup_misses_an_unknown_node_id() {
    let plan = plan_with_storage(STORAGE_NODES);

    assert!(plan.container(NodeId::new(99)).is_none());
}
