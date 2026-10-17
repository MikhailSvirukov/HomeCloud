//! Docker-backed integration test over the `harness-docker` bollard runtime (TST-004).
//!
//! The test is ignored by default so daemon-free runs stay fast; CI executes it
//! in the separate ignored-tests pass against a provided Docker daemon.

mod common;

use harness_docker::{BollardClusterRuntime, ClusterPlan, ClusterRuntime, PlanSettings};
use harness_test_support::topology;

use common::{node_env, plan_order};

/// Image for both nodes; its default command keeps the container running.
const NGINX_IMAGE: &str = "nginx:alpine";
/// Prefix of the per-run network name, so concurrent test runs do not collide.
const NETWORK_PREFIX: &str = "homecloud-test";

/// Builds a 1 dedicated + 1 storage plan on a network unique to this process.
fn plan() -> ClusterPlan {
    let network = format!("{NETWORK_PREFIX}-{}", std::process::id());
    let settings = PlanSettings::new(network, NGINX_IMAGE, NGINX_IMAGE, node_env, None);
    ClusterPlan::from_topology(&topology(1), &settings)
}

#[test]
#[ignore = "requires a Docker daemon; CI runs ignored integration tests"]
fn bollard_runtime_starts_and_removes_a_cluster() {
    let plan = plan();
    let mut runtime =
        BollardClusterRuntime::connect().expect("a Docker daemon is required for this test");

    runtime
        .start(&plan)
        .expect("the planned containers must start on the Docker daemon");

    assert_eq!(
        runtime.running_containers(),
        plan_order(&plan),
        "every planned node is running after start"
    );

    runtime
        .remove(&plan)
        .expect("the started containers and network must be removable");

    assert!(
        runtime.running_containers().is_empty(),
        "no node is running after remove"
    );
}
