//! Docker-free integration tests over the `harness-docker` process runner.
//!
//! These drive [`ProcessCommandRunner`] with ordinary operating-system
//! processes: no Docker daemon or socket is touched. Each test covers exactly
//! one outcome of `CommandRunner::run` — success, a non-zero exit, and a
//! program that cannot be started.

use harness_core::HarnessError;
use harness_docker::{Command, CommandRunner, ProcessCommandRunner};

/// Shell program used to produce controlled exit codes.
const SHELL: &str = "sh";
/// Argument that makes the shell interpret the following string as a script.
const SHELL_COMMAND_FLAG: &str = "-c";
/// Script that ends the shell successfully.
const SUCCESS_SCRIPT: &str = "exit 0";
/// Script that ends the shell with a non-zero exit code.
const FAILURE_SCRIPT: &str = "exit 3";
/// Exit code the failure script reports.
const FAILURE_EXIT_CODE: i32 = 3;
/// Program name that cannot resolve to an executable on any host.
const MISSING_PROGRAM: &str = "homecloud-missing-binary-for-tests-146";

/// Builds a `sh -c <script>` command.
fn shell(script: &str) -> Command {
    Command::new(SHELL, [SHELL_COMMAND_FLAG.to_string(), script.to_string()])
}

/// The error a program exiting with `code` must report.
fn command_failed(program: &str, code: i32) -> HarnessError {
    HarnessError::CommandFailed {
        program: program.to_string(),
        code,
    }
}

/// The error a program that could not be started must report.
fn command_not_started(program: &str) -> HarnessError {
    HarnessError::CommandNotStarted {
        program: program.to_string(),
    }
}

#[test]
fn a_successful_command_returns_ok() {
    let mut runner = ProcessCommandRunner::new();

    let result = runner.run(shell(SUCCESS_SCRIPT));

    assert_eq!(result, Ok(()));
}

#[test]
fn a_failing_command_reports_its_exit_code() {
    let mut runner = ProcessCommandRunner::new();

    let error = runner
        .run(shell(FAILURE_SCRIPT))
        .expect_err("a command exiting non-zero must report an error");

    assert_eq!(error, command_failed(SHELL, FAILURE_EXIT_CODE));
}

#[test]
fn a_missing_program_reports_that_it_could_not_start() {
    let mut runner = ProcessCommandRunner::new();

    let error = runner
        .run(Command::new(MISSING_PROGRAM, Vec::new()))
        .expect_err("a program that does not exist must report an error");

    assert_eq!(error, command_not_started(MISSING_PROGRAM));
}
