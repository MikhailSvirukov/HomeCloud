//! Binary entry point for the dedicated server control plane.
//!
//! Resolves the configuration path from the environment or the default file
//! name, loads and validates the configuration, loads the configured key
//! identity, and reports a one-line topology and identity summary. Networking
//! runtime behavior is added in later issues.

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dedicated_server::config::{ServerConfig, CONFIG_PATH_ENV, DEFAULT_CONFIG_FILE_NAME};
use dedicated_server::identity::ServerIdentity;

// Error context for a configuration load failure.
const CONFIG_CONTEXT: &str = "load configuration";
// Error context for a key identity load failure.
const IDENTITY_CONTEXT: &str = "load key identity";

fn main() -> ExitCode {
    let path = config_path();

    let config = match ServerConfig::load(&path) {
        Ok(config) => config,
        Err(error) => return report_failure(CONFIG_CONTEXT, &path, &error),
    };

    let identity = match ServerIdentity::load(
        &config.identity.label,
        &config.identity.encryption_private_key,
        &config.identity.signing_private_key,
    ) {
        Ok(identity) => identity,
        Err(error) => return report_failure(IDENTITY_CONTEXT, &path, &error),
    };

    print_summary(&config, &identity);
    ExitCode::SUCCESS
}

/// Resolve the configuration path from [`CONFIG_PATH_ENV`] or the default file name.
fn config_path() -> PathBuf {
    match env::var(CONFIG_PATH_ENV) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_CONFIG_FILE_NAME),
    }
}

/// Print the one-line startup summary.
fn print_summary(config: &ServerConfig, identity: &ServerIdentity) {
    println!(
        "dedicated-server listening on {} label={} nodes={} interval_ms={} timeout_ms={} miss_threshold={} encryption_key_bytes={} signing_key_bytes={}",
        config.listen,
        identity.label(),
        config.cluster.nodes.len(),
        config.heartbeat.interval().as_millis(),
        config.heartbeat.timeout().as_millis(),
        config.heartbeat.miss_threshold,
        identity.encryption_private_key().len(),
        identity.signing_private_key().len(),
    );
}

/// Print the error context, `path`, and the error chain to stderr.
fn report_error(context: &str, path: &Path, error: &impl Error) {
    eprintln!(
        "dedicated-server: failed to {context} '{}': {error}",
        path.display()
    );

    let mut source = error.source();
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}

/// Report a failure and return the failure exit code.
fn report_failure(context: &str, path: &Path, error: &impl Error) -> ExitCode {
    report_error(context, path, error);
    ExitCode::FAILURE
}
