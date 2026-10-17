//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads the configuration together with the key identity it names, and
//! logs structured JSON Lines startup and shutdown events to stderr. Networking
//! runtime behavior is added in later issues.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use dedicated_server::{
    init as init_logging, shutdown as log_shutdown, startup as log_startup,
    startup_failed as log_startup_failed, Startup, StartupError, CONFIG_PATH_ENV,
    DEFAULT_CONFIG_FILE_NAME,
};

/// Process allocator tuned for the concurrent workload of a server process
/// rather than the system default.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    init_logging();

    let path = config_path();

    match Startup::load(&path) {
        Ok(startup) => {
            log_startup(&startup.summary());
            log_shutdown();
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

/// Report a startup failure as one error event and return the failure exit code.
fn report_failure(error: &StartupError) -> ExitCode {
    log_startup_failed(error);
    ExitCode::FAILURE
}
