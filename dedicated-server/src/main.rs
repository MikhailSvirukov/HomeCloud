//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads the configuration to learn the structured log path, installs the
//! non-blocking file writer, and then loads the key identity. Startup failures
//! before the writer exists are written directly to stderr; later startup and
//! shutdown events go to the configured file. Networking runtime behavior is
//! added in later issues.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use dedicated_server::{
    init as init_logging, logging_failed_to_stderr, shutdown as log_shutdown,
    startup as log_startup, startup_failed as log_startup_failed, startup_failed_to_stderr,
    ServerConfig, Startup, StartupError, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME,
};

/// Process allocator tuned for the concurrent workload of a server process
/// rather than the system default.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    let path = config_path();
    let config = match ServerConfig::load(&path) {
        Ok(config) => config,
        Err(source) => {
            let error = StartupError::Configuration { path, source };
            let _ = startup_failed_to_stderr(&error);
            return ExitCode::FAILURE;
        }
    };

    let _guard = match init_logging(config.logging_path()) {
        Ok(guard) => guard,
        Err(_) => {
            let _ = logging_failed_to_stderr();
            return ExitCode::FAILURE;
        }
    };

    match Startup::from_config(config) {
        Ok(startup) => {
            log_startup(&startup.summary());
            log_shutdown();
            ExitCode::SUCCESS
        }
        Err(error) => {
            log_startup_failed(&error);
            ExitCode::FAILURE
        }
    }
}

/// Resolve the configuration path from [`CONFIG_PATH_ENV`] or the default file name.
fn config_path() -> PathBuf {
    match env::var(CONFIG_PATH_ENV) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
    }
}
