//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads and validates the configuration, and reports a one-line
//! topology summary. Networking runtime behavior is added in later issues.

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dedicated_server::config::{ServerConfig, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME};

fn main() -> ExitCode {
    let path = config_path();

    match ServerConfig::load(&path) {
        Ok(config) => {
            print_summary(&config);
            ExitCode::SUCCESS
        }
        Err(error) => {
            report_error(&path, &error);
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

/// Print the single-line startup summary to stdout.
fn print_summary(config: &ServerConfig) {
    println!(
        "dedicated-server listening on {} label={} nodes={} interval_ms={} timeout_ms={} miss_threshold={}",
        config.listen,
        config.identity.label,
        config.cluster.nodes.len(),
        config.heartbeat.interval().as_millis(),
        config.heartbeat.timeout().as_millis(),
        config.heartbeat.miss_threshold,
    );
}

/// Print the configuration path and the full error chain to stderr.
fn report_error(path: &Path, error: &impl Error) {
    eprintln!(
        "dedicated-server: failed to load configuration '{}': {error}",
        path.display()
    );

    let mut source = error.source();
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}
