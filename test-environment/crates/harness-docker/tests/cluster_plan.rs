//! Docker-free integration tests over the `harness-docker` public API.
//!
//! These drive the plan and runtime seams exactly as an embedding scenario would:
//! build a validated topology, derive a [`ClusterPlan`], and exercise the in-memory
//! runtime. No Docker daemon or socket is touched.

mod common;

use std::collections::BTreeMap;

use harness_core::{NodeId, NodeRole, NodeSpec};
use harness_docker::{ClusterPlan, ClusterRuntime, FakeClusterRuntime, PlanSettings};
use harness_test_support::addr;

use common::{
    DEDICATED_ID, FIRST_STORAGE_ID, LISTEN_ADDRESS_KEY, NETWORK_NAME, NODE_ID_KEY, NODE_LABEL_KEY,
    NODE_ROLE_KEY, SERVER_IMAGE, STORAGE_IMAGE, STORAGE_NODES, plan_order, plan_with_storage,
    role_value, settings, topology_with_storage,
};

/// Environment key holding the node id in the distinct caller mapping.
const CUSTOM_NODE_ID_KEY: &str = "HC_NODE_ID";
/// Environment key holding the node label in the distinct caller mapping.
const CUSTOM_LABEL_KEY: &str = "HC_LABEL";
/// Environment key holding the node role in the distinct caller mapping.
const CUSTOM_ROLE_KEY: &str = "HC_ROLE";
/// Environment key holding the listen address in the distinct caller mapping.
const CUSTOM_ADDRESS_KEY: &str = "HC_ADDR";
/// Role value rendered for the dedicated server in the distinct caller mapping.
const CUSTOM_DEDICATED_ROLE: &str = "control";
/// Role value rendered for storage nodes in the distinct caller mapping.
const CUSTOM_STORAGE_ROLE: &str = "data";

/// Environment builder with distinct keys and role values, to catch fallbacks.
fn custom_node_env(spec: &NodeSpec) -> BTreeMap<String, String> {
    let role = match spec.role {
        NodeRole::DedicatedServer => CUSTOM_DEDICATED_ROLE,
        NodeRole::Storage => CUSTOM_STORAGE_ROLE,
    };
    BTreeMap::from([
        (CUSTOM_NODE_ID_KEY.to_string(), spec.id.to_string()),
        (CUSTOM_LABEL_KEY.to_string(), spec.label.clone()),
        (CUSTOM_ROLE_KEY.to_string(), role.to_string()),
        (CUSTOM_ADDRESS_KEY.to_string(), spec.address.to_string()),
    ])
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

    assert_eq!(plan.network.name, NETWORK_NAME);
}

#[test]
fn plan_renders_the_settings_env_builder_for_every_container() {
    let plan = plan_with_storage(STORAGE_NODES);

    for container in &plan.containers {
        assert_eq!(
            container.env,
            BTreeMap::from([
                (NODE_ID_KEY.to_string(), container.node.to_string()),
                (NODE_LABEL_KEY.to_string(), container.label.clone()),
                (
                    NODE_ROLE_KEY.to_string(),
                    role_value(container.role).to_string()
                ),
                (
                    LISTEN_ADDRESS_KEY.to_string(),
                    container.address.to_string()
                ),
            ])
        );
    }
}

#[test]
fn plan_forwards_a_distinct_caller_env_mapping_for_every_node() {
    let custom = PlanSettings::new(NETWORK_NAME, SERVER_IMAGE, STORAGE_IMAGE, custom_node_env);
    let plan = ClusterPlan::from_topology(&topology_with_storage(STORAGE_NODES), &custom);

    let dedicated = plan
        .container(NodeId::new(DEDICATED_ID))
        .expect("dedicated-server container is present for id 1");
    assert_eq!(
        dedicated.env,
        BTreeMap::from([
            (CUSTOM_NODE_ID_KEY.to_string(), "1".to_string()),
            (CUSTOM_LABEL_KEY.to_string(), "dedicated".to_string()),
            (
                CUSTOM_ROLE_KEY.to_string(),
                CUSTOM_DEDICATED_ROLE.to_string()
            ),
            (
                CUSTOM_ADDRESS_KEY.to_string(),
                addr(DEDICATED_ID).to_string()
            ),
        ])
    );
    assert!(
        !dedicated.env.contains_key(NODE_ID_KEY),
        "the plan must not fall back to the canonical key names"
    );

    let storage = plan
        .container(NodeId::new(FIRST_STORAGE_ID))
        .expect("first storage container is present for id 2");
    assert_eq!(
        storage.env,
        BTreeMap::from([
            (CUSTOM_NODE_ID_KEY.to_string(), "2".to_string()),
            (CUSTOM_LABEL_KEY.to_string(), "storage-1".to_string()),
            (CUSTOM_ROLE_KEY.to_string(), CUSTOM_STORAGE_ROLE.to_string()),
            (
                CUSTOM_ADDRESS_KEY.to_string(),
                addr(FIRST_STORAGE_ID).to_string()
            ),
        ])
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
        let cluster = topology_with_storage(storage);
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
