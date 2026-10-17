//! Pure descriptions of the isolated Docker test cluster and its container lifecycle.
//!
//! The plan is derived from a validated [`harness_core::ClusterTopology`] and
//! contains no Docker client code, so it can be built and asserted on in fast,
//! daemon-free tests. The runtime seam abstracts container lifecycle operations;
//! [`FakeClusterRuntime`] is the in-memory implementation used by tests.

mod bollard_runtime;
mod cli;
mod command;
mod docker_faults;
mod plan;
mod process;
mod runtime;

pub use bollard_runtime::BollardClusterRuntime;
pub use cli::{CliClusterRuntime, CommandRunner, RecordingCommandRunner};
pub use command::{
    Command, container_kill, container_name, container_remove, container_run, container_start,
    container_stop, network_create, network_remove,
};
pub use docker_faults::DockerFaultInjector;
pub use plan::{ClusterPlan, ContainerPlan, EnvBuilder, NetworkPlan, PlanSettings};
pub use process::ProcessCommandRunner;
pub use runtime::{ClusterRuntime, FakeClusterRuntime};
