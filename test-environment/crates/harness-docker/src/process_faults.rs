//! Process-level fault injection for the storage-node supervisor (TST-016).
//!
//! The storage-node image runs its supervisor as PID 1 with exactly one child
//! process. Killing that child makes the supervisor start a replacement inside
//! the same container; this module observes the PID change and verifies that
//! the container identity did not change.

use std::{
    thread,
    time::{Duration, Instant},
};

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::StreamExt;
use harness_core::{HarnessError, NodeId};
use tokio::runtime::{Builder, Runtime};

use crate::command::container_name;
use crate::plan::ClusterPlan;

type Result<T> = core::result::Result<T, HarnessError>;

const CONNECT: &str = "connect";
const RUNTIME: &str = "runtime";
const INSPECT_CONTAINER: &str = "inspect container";
const CREATE_EXEC: &str = "create exec";
const START_EXEC: &str = "start exec";
const INSPECT_EXEC: &str = "inspect exec";
const EXEC_CAPTURE: &str = "exec capture";
const EXEC: &str = "exec";
const READ_CHILD_PIDS: &str = "read child pids";

const SH: &str = "sh";
const DASH_C: &str = "-c";

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_PROCESS_NAME_BYTES: usize = 15;

/// Lists the supervisor's children and their `/proc/<pid>/comm` names, skipping
/// the shell that `docker exec` created for this command.
///
/// A `docker exec` process is itself a child of PID 1, so the script excludes
/// its own shell PID `$$`; otherwise every read would observe the reader
/// process as a supervised child.
const CHILD_PROCESSES_SCRIPT: &str = r#"for pid in $(cat /proc/1/task/1/children); do [ "$pid" = "$$" ] || printf '%s %s\n' "$pid" "$(cat /proc/$pid/comm 2>/dev/null)"; done"#;

/// The container identity around a supervised process restart (`TST-016`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRestart {
    /// PID of the supervised child before the kill.
    pub previous_pid: u32,
    /// PID of the replacement child.
    pub current_pid: u32,
    /// Container id before and after the restart (must be unchanged).
    pub container_id: String,
    /// Container `StartedAt` before and after the restart (must be unchanged).
    pub started_at: String,
}

/// Kills the supervised child of a running container and waits for its replacement.
pub struct DockerProcessFault {
    plan: ClusterPlan,
    docker: Docker,
    runtime: Runtime,
}

impl DockerProcessFault {
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
        })
    }

    /// Kills the supervised child of `node` whose `/proc/<pid>/comm` equals
    /// `expected_comm` and waits up to `timeout` for its replacement; verifies
    /// the container id and `StartedAt` are unchanged.
    ///
    /// `expected_comm` is the binary name the supervisor runs, at most 15 bytes;
    /// the published default is `storage-node`.
    pub fn restart_node(
        &self,
        node: NodeId,
        expected_comm: &str,
        timeout: Duration,
    ) -> Result<ProcessRestart> {
        validate_process_name(expected_comm)?;

        let container = self
            .plan
            .container(node)
            .ok_or(HarnessError::UnknownNode { id: node })?;
        let name = container_name(&self.plan, container);

        let (container_id, started_at) = self.inspect_identity(&name)?;

        let children = self.read_child_processes(&name)?;
        let previous_pid = children
            .iter()
            .find(|(_, comm)| comm == expected_comm)
            .map(|(pid, _)| *pid)
            .ok_or(HarnessError::SupervisedProcessMissing { node })?;

        self.exec_ok(&name, &exec_argv(&kill_process_script(expected_comm)))?;

        let started = Instant::now();
        loop {
            let children = self.read_child_processes(&name)?;
            if let Some((current_pid, _)) = children
                .iter()
                .find(|(pid, comm)| *pid != previous_pid && comm == expected_comm)
            {
                let (current_container_id, current_started_at) = self.inspect_identity(&name)?;
                if current_container_id != container_id || current_started_at != started_at {
                    return Err(HarnessError::ContainerRecreated { node });
                }
                return Ok(ProcessRestart {
                    previous_pid,
                    current_pid: *current_pid,
                    container_id,
                    started_at,
                });
            }

            if started.elapsed() >= timeout {
                return Err(HarnessError::SupervisedProcessRestartTimedOut { node, previous_pid });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn inspect_identity(&self, container: &str) -> Result<(String, String)> {
        let inspected = self
            .runtime
            .block_on(self.docker.inspect_container(container, None))
            .map_err(|error| docker_failure(INSPECT_CONTAINER, error))?;

        let id = inspected.id.ok_or_else(|| HarnessError::DockerApiFailed {
            operation: INSPECT_CONTAINER,
            message: format!("container {container} has no id"),
        })?;
        let started_at = inspected
            .state
            .and_then(|state| state.started_at)
            .ok_or_else(|| HarnessError::DockerApiFailed {
                operation: INSPECT_CONTAINER,
                message: format!("container {container} has no StartedAt"),
            })?;

        Ok((id, started_at))
    }

    fn read_child_processes(&self, container: &str) -> Result<Vec<(u32, String)>> {
        let (code, stdout) = self.exec_capture(container, &exec_argv(CHILD_PROCESSES_SCRIPT))?;
        if code != 0 {
            return Err(HarnessError::DockerApiFailed {
                operation: READ_CHILD_PIDS,
                message: format!("child-list command exited with code {code}"),
            });
        }
        parse_child_processes(&stdout)
    }

    fn exec_ok(&self, container: &str, argv: &[String]) -> Result<()> {
        let (code, _) = self.exec_capture(container, argv)?;
        if code == 0 {
            Ok(())
        } else {
            Err(HarnessError::DockerApiFailed {
                operation: EXEC,
                message: format!("{} exited with code {code}", argv.join(" ")),
            })
        }
    }

    fn exec_capture(&self, container: &str, argv: &[String]) -> Result<(i64, String)> {
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

        let stdout: Vec<u8> = self.runtime.block_on(async {
            let mut stdout = Vec::new();
            while let Some(chunk) = output.next().await {
                let chunk = chunk.map_err(|error| docker_failure(EXEC_CAPTURE, error))?;
                if let LogOutput::StdOut { message } = chunk {
                    stdout.extend_from_slice(&message);
                }
            }
            Ok(stdout)
        })?;

        let inspected = self
            .runtime
            .block_on(self.docker.inspect_exec(&created.id))
            .map_err(|error| docker_failure(INSPECT_EXEC, error))?;
        let code = inspected
            .exit_code
            .ok_or_else(|| HarnessError::DockerApiFailed {
                operation: INSPECT_EXEC,
                message: "exec exit code is unavailable".to_string(),
            })?;
        let stdout = String::from_utf8(stdout).map_err(|error| HarnessError::DockerApiFailed {
            operation: EXEC_CAPTURE,
            message: format!("exec output is not UTF-8: {error}"),
        })?;

        Ok((code, stdout))
    }
}

fn docker_failure(operation: &'static str, error: bollard::errors::Error) -> HarnessError {
    HarnessError::DockerApiFailed {
        operation,
        message: error.to_string(),
    }
}

fn exec_argv(script: &str) -> Vec<String> {
    vec![SH.to_string(), DASH_C.to_string(), script.to_string()]
}

fn validate_process_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_PROCESS_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));

    if valid {
        Ok(())
    } else {
        Err(HarnessError::InvalidProcessName {
            name: name.to_string(),
        })
    }
}

fn kill_process_script(comm: &str) -> String {
    format!(
        r#"for pid in $(cat /proc/1/task/1/children); do [ "$pid" = "$$" ] && continue; [ "$pid" = "1" ] && continue; [ "$(cat /proc/$pid/comm 2>/dev/null)" = "{comm}" ] && kill -KILL "$pid"; done"#
    )
}

fn parse_child_processes(output: &str) -> Result<Vec<(u32, String)>> {
    let mut children = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (pid_token, comm) = match line.split_once(|character: char| character.is_whitespace()) {
            Some((pid, comm)) => (pid, comm),
            None => (line, ""),
        };

        let pid = pid_token
            .parse::<u32>()
            .map_err(|_| HarnessError::DockerApiFailed {
                operation: READ_CHILD_PIDS,
                message: format!("invalid child pid {pid_token:?}"),
            })?;

        let comm = comm.trim();
        if comm.is_empty() {
            continue;
        }

        children.push((pid, comm.to_string()));
    }

    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::{
        CHILD_PROCESSES_SCRIPT, exec_argv, kill_process_script, parse_child_processes,
        validate_process_name,
    };
    use harness_core::HarnessError;

    #[test]
    fn parses_child_processes_with_names() {
        assert_eq!(
            parse_child_processes("123 sleep\n456 storage-node\n"),
            Ok(vec![
                (123, "sleep".to_string()),
                (456, "storage-node".to_string()),
            ])
        );
        assert_eq!(parse_child_processes(""), Ok(Vec::new()));
        assert_eq!(parse_child_processes("789 \n"), Ok(Vec::new()));
    }

    #[test]
    fn rejects_non_numeric_child_pid() {
        assert_eq!(
            parse_child_processes("abc sleep\n"),
            Err(HarnessError::DockerApiFailed {
                operation: "read child pids",
                message: "invalid child pid \"abc\"".to_string(),
            })
        );
    }

    #[test]
    fn rejects_invalid_process_names() {
        for name in ["", "0123456789abcdef", "bad name", "a;b"] {
            assert_eq!(
                validate_process_name(name),
                Err(HarnessError::InvalidProcessName {
                    name: name.to_string(),
                })
            );
        }
        assert_eq!(validate_process_name("storage-node"), Ok(()));
    }

    #[test]
    fn kill_script_injects_the_validated_process_name() {
        let script = kill_process_script("storage-node");
        assert!(script.contains(r#"[ "$(cat /proc/$pid/comm 2>/dev/null)" = "storage-node" ]"#));
        assert!(script.contains(r#"[ "$pid" = "$$" ] && continue"#));
        assert!(script.contains(r#"[ "$pid" = "1" ] && continue"#));
    }

    #[test]
    fn builds_sh_c_argv_for_child_process_script() {
        assert_eq!(
            exec_argv(CHILD_PROCESSES_SCRIPT),
            vec![
                "sh".to_string(),
                "-c".to_string(),
                CHILD_PROCESSES_SCRIPT.to_string()
            ]
        );
    }
}
