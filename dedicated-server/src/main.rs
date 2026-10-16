//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads the configuration together with the key identity it names, and
//! reports a one-line topology and identity summary. Networking runtime
//! behavior is added in later issues.

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use dedicated_server::{Startup, StartupError, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME};

fn main() -> ExitCode {
    let path = config_path();

    match Startup::load(&path) {
        Ok(startup) => {
            println!("{}", startup.summary());
            ExitCode::SUCCESS
        }
        Err(error) => report_failure(&error),
    }
}

/// Resolve the configuration path from [`CONFIG_PATH_ENV`] or the default file name.
fn config_path() -> PathBuf {
    match env::var(CONFIG_PATH_ENV) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
    }
}

/// Report a startup failure to stderr and return the failure exit code.
///
/// The startup error renders its immediate cause on the first line, so the
/// `caused by:` chain continues from the cause of that cause.
fn report_failure(error: &StartupError) -> ExitCode {
    eprintln!("dedicated-server: {error}");

    let mut cause = error.source().and_then(|source| source.source());
    while let Some(current) = cause {
        eprintln!("  caused by: {current}");
        cause = current.source();
    }

    ExitCode::FAILURE
}
