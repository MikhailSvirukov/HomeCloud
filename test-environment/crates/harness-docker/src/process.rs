//! A command runner that executes commands with the operating system.

use std::process::Command as ProcessCommand;

use harness_core::HarnessError;

use crate::cli::CommandRunner;
use crate::command::Command;

/// `CommandFailed` code used when a process ends without an exit code.
const NO_EXIT_CODE: i32 = -1;

/// A [`CommandRunner`] that runs each command as a child process.
#[derive(Default)]
pub struct ProcessCommandRunner;

impl ProcessCommandRunner {
    /// Creates a runner that executes commands as child processes.
    pub fn new() -> Self {
        Self
    }
}

impl CommandRunner for ProcessCommandRunner {
    /// Runs `command`, mapping a missing program to
    /// [`HarnessError::CommandNotStarted`] and a failure exit to
    /// [`HarnessError::CommandFailed`].
    fn run(&mut self, command: Command) -> Result<(), HarnessError> {
        let program = command.program;
        match ProcessCommand::new(&program).args(&command.args).status() {
            Ok(status) => match status.success() {
                true => Ok(()),
                false => Err(HarnessError::CommandFailed {
                    program,
                    code: status.code().unwrap_or(NO_EXIT_CODE),
                }),
            },
            Err(_) => Err(HarnessError::CommandNotStarted { program }),
        }
    }
}
