//! Docker API runtime backed by the `bollard` container-management library (TST-004).

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, NetworkCreateRequest, NetworkingConfig,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, KillContainerOptionsBuilder,
    RemoveContainerOptionsBuilder, StartContainerOptionsBuilder, StopContainerOptionsBuilder,
};
use futures_util::StreamExt;
use harness_core::{HarnessError, NodeId};
use tokio::runtime::{Builder, Runtime};

use crate::command::container_name;
use crate::plan::{ClusterPlan, ContainerPlan};
use crate::runtime::ClusterRuntime;

type Result<T> = core::result::Result<T, HarnessError>;

const CONNECT: &str = "connect";
const RUNTIME: &str = "runtime";
const NETWORK_CREATE: &str = "create network";
const NETWORK_REMOVE: &str = "remove network";
const IMAGE_INSPECT: &str = "inspect image";
const IMAGE_PULL: &str = "pull image";
const CONTAINER_CREATE: &str = "create container";
const CONTAINER_START: &str = "start container";
const CONTAINER_STOP: &str = "stop container";
const CONTAINER_KILL: &str = "kill container";
const CONTAINER_REMOVE: &str = "remove container";
const IMAGE_NOT_FOUND: u16 = 404;

fn docker_failure(operation: &'static str, error: bollard::errors::Error) -> HarnessError {
    HarnessError::DockerApiFailed {
        operation,
        message: error.to_string(),
    }
}

/// A [`ClusterRuntime`] that manages the cluster through the Docker API.
///
/// The bollard client is asynchronous, so every call is driven to completion on
/// a private current-thread Tokio runtime. State mirrors the CLI runtime:
/// network creation, created containers, and running containers are tracked so
/// repeated lifecycle calls stay idempotent.
pub struct BollardClusterRuntime {
    docker: Docker,
    runtime: Runtime,
    running: Vec<NodeId>,
    created: Vec<NodeId>,
    network_created: bool,
}

impl BollardClusterRuntime {
    /// Connects to the local Docker daemon.
    pub fn connect() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|error| docker_failure(CONNECT, error))?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| HarnessError::DockerApiFailed {
                operation: RUNTIME,
                message: error.to_string(),
            })?;
        Ok(Self {
            docker,
            runtime,
            running: Vec::new(),
            created: Vec::new(),
            network_created: false,
        })
    }

    fn create_network(&self, plan: &ClusterPlan) -> Result<()> {
        let request = NetworkCreateRequest {
            name: plan.network.name.clone(),
            ..Default::default()
        };
        self.runtime
            .block_on(self.docker.create_network(request))
            .map(|_| ())
            .map_err(|error| docker_failure(NETWORK_CREATE, error))
    }

    fn ensure_image(&self, image: &str) -> Result<()> {
        match self.runtime.block_on(self.docker.inspect_image(image)) {
            Ok(_) => Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: IMAGE_NOT_FOUND,
                ..
            }) => self.pull_image(image),
            Err(error) => Err(docker_failure(IMAGE_INSPECT, error)),
        }
    }

    fn pull_image(&self, image: &str) -> Result<()> {
        let options = CreateImageOptionsBuilder::new().from_image(image).build();
        self.runtime.block_on(async {
            let mut stream = self.docker.create_image(Some(options), None, None);
            while let Some(event) = stream.next().await {
                event.map_err(|error| docker_failure(IMAGE_PULL, error))?;
            }
            Ok(())
        })
    }

    fn create_container(&self, plan: &ClusterPlan, container: &ContainerPlan) -> Result<()> {
        let name = container_name(plan, container);
        let options = CreateContainerOptionsBuilder::new().name(&name).build();
        let body = ContainerCreateBody {
            image: Some(container.image.clone()),
            hostname: Some(container.hostname.clone()),
            env: Some(
                container
                    .env
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect(),
            ),
            host_config: Some(host_config(plan, container)),
            networking_config: Some(networking_config(plan, container)),
            ..Default::default()
        };
        self.runtime
            .block_on(self.docker.create_container(Some(options), body))
            .map(|_| ())
            .map_err(|error| docker_failure(CONTAINER_CREATE, error))
    }

    fn start_container(&self, plan: &ClusterPlan, container: &ContainerPlan) -> Result<()> {
        let name = container_name(plan, container);
        let options = StartContainerOptionsBuilder::new().build();
        self.runtime
            .block_on(self.docker.start_container(&name, Some(options)))
            .map_err(|error| docker_failure(CONTAINER_START, error))
    }

    fn stop_container(&self, plan: &ClusterPlan, container: &ContainerPlan) -> Result<()> {
        let name = container_name(plan, container);
        let options = StopContainerOptionsBuilder::new().build();
        self.runtime
            .block_on(self.docker.stop_container(&name, Some(options)))
            .map_err(|error| docker_failure(CONTAINER_STOP, error))
    }

    fn kill_container(&self, plan: &ClusterPlan, container: &ContainerPlan) -> Result<()> {
        let name = container_name(plan, container);
        let options = KillContainerOptionsBuilder::new().build();
        self.runtime
            .block_on(self.docker.kill_container(&name, Some(options)))
            .map_err(|error| docker_failure(CONTAINER_KILL, error))
    }

    fn remove_container(&self, plan: &ClusterPlan, container: &ContainerPlan) -> Result<()> {
        let name = container_name(plan, container);
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        self.runtime
            .block_on(self.docker.remove_container(&name, Some(options)))
            .map_err(|error| docker_failure(CONTAINER_REMOVE, error))
    }

    fn remove_network(&self, plan: &ClusterPlan) -> Result<()> {
        self.runtime
            .block_on(self.docker.remove_network(&plan.network.name))
            .map_err(|error| docker_failure(NETWORK_REMOVE, error))
    }
}

impl ClusterRuntime for BollardClusterRuntime {
    fn start(&mut self, plan: &ClusterPlan) -> Result<()> {
        if !self.network_created {
            self.create_network(plan)?;
            self.network_created = true;
        }
        for container in &plan.containers {
            let node = container.node;
            match self.created.contains(&node) {
                false => {
                    self.ensure_image(&container.image)?;
                    self.create_container(plan, container)?;
                    self.start_container(plan, container)?;
                    self.created.push(node);
                    self.running.push(node);
                }
                true if !self.running.contains(&node) => {
                    self.start_container(plan, container)?;
                    self.running.push(node);
                }
                true => {}
            }
        }
        Ok(())
    }

    fn stop(&mut self, plan: &ClusterPlan) -> Result<()> {
        for node in &self.running {
            if let Some(container) = plan.container(*node) {
                self.stop_container(plan, container)?;
            }
        }
        self.running.clear();
        Ok(())
    }

    fn kill(&mut self, plan: &ClusterPlan, node: NodeId) -> Result<()> {
        match plan.container(node) {
            Some(container) if self.running.contains(&node) => {
                self.kill_container(plan, container)?;
                self.running.retain(|running| *running != node);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn remove(&mut self, plan: &ClusterPlan) -> Result<()> {
        for container in &plan.containers {
            if self.created.contains(&container.node) {
                self.remove_container(plan, container)?;
            }
        }
        if self.network_created {
            self.remove_network(plan)?;
            self.network_created = false;
        }
        self.created.clear();
        self.running.clear();
        Ok(())
    }

    fn running_containers(&self) -> &[NodeId] {
        &self.running
    }
}

fn host_config(plan: &ClusterPlan, container: &ContainerPlan) -> HostConfig {
    let binds = container
        .data_volume_path
        .as_ref()
        .map(|path| vec![format!("{}-data:{path}", container_name(plan, container))]);
    HostConfig {
        binds,
        ..Default::default()
    }
}

fn networking_config(plan: &ClusterPlan, container: &ContainerPlan) -> NetworkingConfig {
    let endpoint = EndpointSettings {
        aliases: Some(vec![container.hostname.clone()]),
        ..Default::default()
    };
    NetworkingConfig {
        endpoints_config: Some(HashMap::from([(plan.network.name.clone(), endpoint)])),
    }
}
