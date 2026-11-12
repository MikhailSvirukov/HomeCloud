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
const ZOMBIE_STATE: &str = "Z";
const DEAD_STATE: &str = "X";

/// Lists the supervisor's children with their `/proc/<pid>/comm` name and
/// `/proc/<pid>/stat` state, skipping the shell that `docker exec` created for
/// this command.
///
/// A `docker exec` process is itself a child of PID 1, so the script excludes
/// its own shell PID `$$`; otherwise every read would observe the reader
/// process as a supervised child. The state is extracted after the final `)`
/// in the stat line so a comm containing spaces does not shift the fields.
const CHILD_PROCESSES_SCRIPT: &str = r#"for pid in $(cat /proc/1/task/1/children); do
  [ "$pid" = "$$" ] && continue
  comm=$(cat /proc/$pid/comm 2>/dev/null) || continue
  stat=$(cat /proc/$pid/stat 2>/dev/null) || continue
  rest=${stat#*) }
  state=${rest%% *}
  printf '%s %s %s\n' "$pid" "$comm" "$state"
done"#;

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
        let previous_pid = matching_pid(&children, expected_comm, None)
            .ok_or(HarnessError::SupervisedProcessMissing { node })?;

        self.exec_ok(
            &name,
            &exec_argv(&kill_process_script(previous_pid, expected_comm)),
        )?;

        let started = Instant::now();
        loop {
            let children = self.read_child_processes(&name)?;
            if let Some(current_pid) = matching_pid(&children, expected_comm, Some(previous_pid)) {
                let (current_container_id, current_started_at) = self.inspect_identity(&name)?;
                if current_container_id != container_id || current_started_at != started_at {
                    return Err(HarnessError::ContainerRecreated { node });
                }
                return Ok(ProcessRestart {
                    previous_pid,
                    current_pid,
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

    fn read_child_processes(&self, container: &str) -> Result<Vec<(u32, String, String)>> {
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

fn kill_process_script(pid: u32, comm: &str) -> String {
    format!(
        r#"[ "$(cat /proc/{pid}/comm 2>/dev/null)" = "{comm}" ] && kill -KILL {pid}
exit 0"#
    )
}

fn parse_child_processes(output: &str) -> Result<Vec<(u32, String, String)>> {
    let mut children = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (pid_token, rest) = match line.split_once(|character: char| character.is_whitespace()) {
            Some((pid, rest)) => (pid, rest),
            None => (line, ""),
        };

        let pid = pid_token
            .parse::<u32>()
            .map_err(|_| HarnessError::DockerApiFailed {
                operation: READ_CHILD_PIDS,
                message: format!("invalid child pid {pid_token:?}"),
            })?;

        let Some((comm, state)) = rest.rsplit_once(|character: char| character.is_whitespace())
        else {
            continue;
        };

        if comm.is_empty() || state.is_empty() {
            continue;
        }

        children.push((pid, comm.to_string(), state.to_string()));
    }

    Ok(children)
}

fn is_running_child(state: &str) -> bool {
    state != ZOMBIE_STATE && state != DEAD_STATE
}

fn matching_pid(
    children: &[(u32, String, String)],
    comm: &str,
    exclude: Option<u32>,
) -> Option<u32> {
    children
        .iter()
        .find(|(pid, child_comm, state)| {
            Some(*pid) != exclude && child_comm == comm && is_running_child(state)
        })
        .map(|(pid, _, _)| *pid)
}

#[cfg(test)]
mod tests {
    use super::{
        CHILD_PROCESSES_SCRIPT, exec_argv, kill_process_script, matching_pid,
        parse_child_processes, validate_process_name,
    };
    use harness_core::HarnessError;

    #[test]
    fn parses_child_processes_with_state() {
        assert_eq!(
            parse_child_processes("123 sleep S\n456 storage-node R\n"),
            Ok(vec![
                (123, "sleep".to_string(), "S".to_string()),
                (456, "storage-node".to_string(), "R".to_string()),
            ])
        );
        assert_eq!(parse_child_processes(""), Ok(Vec::new()));
        assert_eq!(parse_child_processes("789 \n"), Ok(Vec::new()));
    }

    #[test]
    fn ignores_malformed_and_partial_child_lines() {
        assert_eq!(
            parse_child_processes("123 sleep\n456\n789  S\n"),
            Ok(Vec::new())
        );
    }

    #[test]
    fn rejects_non_numeric_child_pid() {
        assert_eq!(
            parse_child_processes("abc sleep S\n"),
            Err(HarnessError::DockerApiFailed {
                operation: "read child pids",
                message: "invalid child pid \"abc\"".to_string(),
            })
        );
    }

    #[test]
    fn ignores_zombie_dead_and_mismatched_children() {
        let children = vec![
            (10, "storage-node".to_string(), "Z".to_string()),
            (11, "storage-node".to_string(), "X".to_string()),
            (12, "sleep".to_string(), "S".to_string()),
            (13, "storage-node".to_string(), "S".to_string()),
        ];

        assert_eq!(matching_pid(&children, "storage-node", None), Some(13));
        assert_eq!(matching_pid(&children, "missing", None), None);
        assert_eq!(matching_pid(&children, "storage-node", Some(13)), None);
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
    fn kill_script_targets_the_observed_pid_and_comm() {
        assert_eq!(
            kill_process_script(42, "storage-node"),
            r#"[ "$(cat /proc/42/comm 2>/dev/null)" = "storage-node" ] && kill -KILL 42
exit 0"#
        );
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
