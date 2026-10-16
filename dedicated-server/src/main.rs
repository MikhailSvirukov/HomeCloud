//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads the configuration together with the key identity it names, and
//! logs a one-line topology and identity summary to stderr. Networking runtime
//! behavior is added in later issues.

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use dedicated_server::{Startup, StartupError, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME};

/// Log level used when `RUST_LOG` is unset or cannot be parsed.
const DEFAULT_LOG_FILTER: &str = "info";

fn main() -> ExitCode {
    init_logging();

    let path = config_path();

    match Startup::load(&path) {
        Ok(startup) => {
            info!("{}", startup.summary());
            ExitCode::SUCCESS
        }
        Err(error) => report_failure(&error),
    }
}

/// Install a stderr logger filtered by `RUST_LOG`, defaulting to [`DEFAULT_LOG_FILTER`].
///
/// ANSI colours are disabled so the output is stable when captured by tests and
/// log collectors; stdout stays free for the process's own output.
fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
}

/// Resolve the configuration path from [`CONFIG_PATH_ENV`] or the default file name.
fn config_path() -> PathBuf {
    match env::var(CONFIG_PATH_ENV) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
    }
}

/// Report a startup failure as one error event and return the failure exit code.
///
/// The startup error renders its immediate cause on the first line, so the
/// `caused by:` chain continues from the cause of that cause.
fn report_failure(error: &StartupError) -> ExitCode {
    error!("{}", failure_message(error));
    ExitCode::FAILURE
}

/// Render the startup error together with its remaining `caused by:` chain.
fn failure_message(error: &StartupError) -> String {
    let mut message = error.to_string();
    let mut cause = error.source().and_then(|source| source.source());
    while let Some(current) = cause {
        message.push_str("\n  caused by: ");
        message.push_str(&current.to_string());
        cause = current.source();
    }
    message
}
