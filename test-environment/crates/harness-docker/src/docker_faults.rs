//! Docker-level selective network fault injection (TST-006).

use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::StreamExt;
use harness_core::{Fault, FaultInjector, HarnessError, LossPercent, PeerLink};
use tokio::runtime::{Builder, Runtime};

use crate::command::container_name;
use crate::plan::{ClusterPlan, ContainerPlan};

type Result<T> = core::result::Result<T, HarnessError>;

const CONNECT: &str = "connect";
const RUNTIME: &str = "runtime";
const INSPECT_CONTAINER: &str = "inspect container";
const CREATE_EXEC: &str = "create exec";
const START_EXEC: &str = "start exec";
const INSPECT_EXEC: &str = "inspect exec";
const EXEC: &str = "exec";

const IPTABLES: &str = "iptables";
const WAIT: &str = "-w";
const APPEND: &str = "-A";
const DELETE: &str = "-D";
const OUTPUT: &str = "OUTPUT";
const INPUT: &str = "INPUT";
const DESTINATION: &str = "-d";
const SOURCE: &str = "-s";
const MATCH: &str = "-m";
const STATISTIC: &str = "statistic";
const MODE: &str = "--mode";
const RANDOM: &str = "random";
const PROBABILITY: &str = "--probability";
const JUMP: &str = "-j";
const DROP: &str = "DROP";

fn docker_failure(operation: &'static str, error: bollard::errors::Error) -> HarnessError {
    HarnessError::DockerApiFailed {
        operation,
        message: error.to_string(),
    }
}

/// A [`FaultInjector`] that applies per-pair packet filtering inside the node
/// containers through the Docker API.
pub struct DockerFaultInjector {
    plan: ClusterPlan,
    docker: Docker,
    runtime: Runtime,
    active: Vec<Fault>,
}

impl DockerFaultInjector {
    /// Connects to the Docker daemon for the given plan.
    pub fn connect(plan: ClusterPlan) -> Result<Self> {
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
            plan,
            docker,
            runtime,
            active: Vec::new(),
        })
    }

    fn ensure_known(&self, fault: &Fault) -> Result<()> {
        match fault {
            Fault::NodeShutdown { node } => self
                .plan
                .container(*node)
                .map(|_| ())
                .ok_or(HarnessError::UnknownNode { id: *node }),
            Fault::LinkDegradation { link, .. } => self.link_containers(*link).map(|_| ()),
        }
    }

    fn link_containers(&self, link: PeerLink) -> Result<(&ContainerPlan, &ContainerPlan)> {
        let (a, b) = (link.a(), link.b());
        match (self.plan.container(a), self.plan.container(b)) {
            (Some(first), Some(second)) => Ok((first, second)),
            _ => Err(HarnessError::UnknownPeerLink { a, b }),
        }
    }

    fn container_ip(&self, container: &ContainerPlan) -> Result<String> {
        let name = container_name(&self.plan, container);
        let inspected = self
            .runtime
            .block_on(self.docker.inspect_container(&name, None))
            .map_err(|error| docker_failure(INSPECT_CONTAINER, error))?;
        inspected
            .network_settings
            .and_then(|settings| settings.networks)
            .and_then(|networks| networks.get(&self.plan.network.name).cloned())
            .and_then(|endpoint| endpoint.ip_address)
            .filter(|address| !address.is_empty())
            .ok_or_else(|| HarnessError::DockerApiFailed {
                operation: INSPECT_CONTAINER,
                message: format!(
                    "container {name} has no address on network {}",
                    self.plan.network.name
                ),
            })
    }

    fn apply_rules(&self, container: &ContainerPlan, rules: &[Vec<String>]) -> Result<()> {
        let name = container_name(&self.plan, container);
        rules.iter().try_for_each(|argv| self.exec(&name, argv))
    }

    fn exec(&self, container: &str, argv: &[String]) -> Result<()> {
        let config = CreateExecOptions {
            cmd: Some(argv.to_vec()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        let created = self
            .runtime
            .block_on(self.docker.create_exec(container, config))
            .map_err(|error| docker_failure(CREATE_EXEC, error))?;
        let started = self
            .runtime
            .block_on(self.docker.start_exec(&created.id, None))
            .map_err(|error| docker_failure(START_EXEC, error))?;
        let StartExecResults::Attached { mut output, .. } = started else {
            return Err(HarnessError::DockerApiFailed {
                operation: START_EXEC,
                message: "exec started detached".to_string(),
            });
        };
        self.runtime.block_on(async {
            while let Some(chunk) = output.next().await {
                chunk.map_err(|error| docker_failure(EXEC, error))?;
            }
            Ok(())
        })?;
        let inspected = self
            .runtime
            .block_on(self.docker.inspect_exec(&created.id))
            .map_err(|error| docker_failure(INSPECT_EXEC, error))?;
        match inspected.exit_code {
            Some(0) => Ok(()),
            Some(code) => Err(HarnessError::DockerApiFailed {
                operation: EXEC,
                message: format!("{} exited with code {code}", argv.join(" ")),
            }),
            None => Err(HarnessError::DockerApiFailed {
                operation: INSPECT_EXEC,
                message: "exec exit code is unavailable".to_string(),
            }),
        }
    }

    fn apply_link(&self, link: PeerLink, loss: LossPercent) -> Result<()> {
        let percent = u8::from(loss);
        let (first, second) = self.link_containers(link)?;
        let first_ip = self.container_ip(first)?;
        let second_ip = self.container_ip(second)?;
        self.apply_rules(first, &filter_rules(APPEND, &second_ip, percent))?;
        self.apply_rules(second, &filter_rules(APPEND, &first_ip, percent))
    }

    fn revert_link(&self, link: PeerLink, loss: LossPercent) -> Result<()> {
        let percent = u8::from(loss);
        let (first, second) = self.link_containers(link)?;
        let first_ip = self.container_ip(first)?;
        let second_ip = self.container_ip(second)?;
        self.apply_rules(first, &filter_rules(DELETE, &second_ip, percent))?;
        self.apply_rules(second, &filter_rules(DELETE, &first_ip, percent))
    }
}

impl FaultInjector for DockerFaultInjector {
    fn apply(&mut self, fault: &Fault) -> Result<()> {
        if self.active.contains(fault) {
            return Err(HarnessError::FaultAlreadyActive {
                fault: fault.clone(),
            });
        }
        self.ensure_known(fault)?;
        match fault {
            Fault::NodeShutdown { .. } => {}
            Fault::LinkDegradation { link, conditions } => {
                self.apply_link(*link, conditions.loss_percent)?;
            }
        }
        self.active.push(fault.clone());
        Ok(())
    }

    fn revert(&mut self, fault: &Fault) -> Result<()> {
        let Some(index) = self.active.iter().position(|active| active == fault) else {
            return Err(HarnessError::FaultNotActive {
                fault: fault.clone(),
            });
        };
        match fault {
            Fault::NodeShutdown { .. } => {}
            Fault::LinkDegradation { link, conditions } => {
                self.revert_link(*link, conditions.loss_percent)?;
            }
        }
        self.active.remove(index);
        Ok(())
    }

    fn active_faults(&self) -> &[Fault] {
        &self.active
    }
}

fn filter_rules(operation: &str, peer: &str, loss: u8) -> Vec<Vec<String>> {
    match loss {
        0 => Vec::new(),
        100 => vec![
            rule(operation, OUTPUT, DESTINATION, peer),
            rule(operation, INPUT, SOURCE, peer),
        ],
        _ => {
            let probability = (f64::from(loss) / 100.0).to_string();
            vec![
                throttled_rule(operation, OUTPUT, DESTINATION, peer, &probability),
                throttled_rule(operation, INPUT, SOURCE, peer, &probability),
            ]
        }
    }
}

fn rule(operation: &str, chain: &str, flag: &str, peer: &str) -> Vec<String> {
    vec![
        IPTABLES.to_string(),
        WAIT.to_string(),
        operation.to_string(),
        chain.to_string(),
        flag.to_string(),
        peer.to_string(),
        JUMP.to_string(),
        DROP.to_string(),
    ]
}

fn throttled_rule(
    operation: &str,
    chain: &str,
    flag: &str,
    peer: &str,
    probability: &str,
) -> Vec<String> {
    vec![
        IPTABLES.to_string(),
        WAIT.to_string(),
        operation.to_string(),
        chain.to_string(),
        flag.to_string(),
        peer.to_string(),
        MATCH.to_string(),
        STATISTIC.to_string(),
        MODE.to_string(),
        RANDOM.to_string(),
        PROBABILITY.to_string(),
        probability.to_string(),
        JUMP.to_string(),
        DROP.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn filter_rules_target_the_peer_in_both_directions() {
        assert_eq!(
            filter_rules(APPEND, "10.0.0.2", 100),
            vec![
                argv(&[
                    "iptables", "-w", "-A", "OUTPUT", "-d", "10.0.0.2", "-j", "DROP"
                ]),
                argv(&[
                    "iptables", "-w", "-A", "INPUT", "-s", "10.0.0.2", "-j", "DROP"
                ]),
            ],
        );
        assert_eq!(
            filter_rules(DELETE, "10.0.0.2", 50),
            vec![
                argv(&[
                    "iptables",
                    "-w",
                    "-D",
                    "OUTPUT",
                    "-d",
                    "10.0.0.2",
                    "-m",
                    "statistic",
                    "--mode",
                    "random",
                    "--probability",
                    "0.5",
                    "-j",
                    "DROP",
                ]),
                argv(&[
                    "iptables",
                    "-w",
                    "-D",
                    "INPUT",
                    "-s",
                    "10.0.0.2",
                    "-m",
                    "statistic",
                    "--mode",
                    "random",
                    "--probability",
                    "0.5",
                    "-j",
                    "DROP",
                ]),
            ],
        );
    }
}
